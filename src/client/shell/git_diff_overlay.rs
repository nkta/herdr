use super::*;

struct SideBySideCell {
    lineno: u32,
    text: String,
    changed: bool,
}

#[derive(Default)]
struct SideBySideRow {
    left: Option<SideBySideCell>,
    right: Option<SideBySideCell>,
}

/// Interleaves a hunk's lines into aligned side-by-side rows: context lines occupy both columns
/// on the same row; a contiguous deletion block and the addition block that follows it are
/// zipped row-by-row, with the shorter side left blank for any extra rows on the longer side.
///
/// Ported from the pre-client-shell Git panel's `workspace::git::diff::hunk_to_side_by_side`
/// (removed along with the rest of that panel by an upstream merge): this is presentation
/// formatting, so it now lives on the client instead of the server.
fn hunk_to_side_by_side(hunk: &crate::api::schema::GitDiffHunk) -> Vec<SideBySideRow> {
    use crate::api::schema::GitDiffLineKind;

    let mut rows = Vec::new();
    let mut i = 0;

    while i < hunk.lines.len() {
        let line = &hunk.lines[i];
        match line.kind {
            GitDiffLineKind::Context => {
                rows.push(SideBySideRow {
                    left: Some(SideBySideCell {
                        lineno: line.old_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: false,
                    }),
                    right: Some(SideBySideCell {
                        lineno: line.new_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: false,
                    }),
                });
                i += 1;
            }
            GitDiffLineKind::Deletion | GitDiffLineKind::Addition => {
                let deletions_start = i;
                while i < hunk.lines.len() && hunk.lines[i].kind == GitDiffLineKind::Deletion {
                    i += 1;
                }
                let deletions = &hunk.lines[deletions_start..i];

                let additions_start = i;
                while i < hunk.lines.len() && hunk.lines[i].kind == GitDiffLineKind::Addition {
                    i += 1;
                }
                let additions = &hunk.lines[additions_start..i];

                let paired = deletions.len().max(additions.len());
                for j in 0..paired {
                    let left = deletions.get(j).map(|line| SideBySideCell {
                        lineno: line.old_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: true,
                    });
                    let right = additions.get(j).map(|line| SideBySideCell {
                        lineno: line.new_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: true,
                    });
                    rows.push(SideBySideRow { left, right });
                }
            }
            GitDiffLineKind::Unknown => {
                i += 1;
            }
        }
    }

    rows
}

fn all_rows(diff: &crate::api::schema::GitFileDiff) -> Vec<SideBySideRow> {
    diff.hunks.iter().flat_map(hunk_to_side_by_side).collect()
}

fn render_side_by_side_cell(
    buffer: &mut Buffer,
    area: Rect,
    cell: Option<&SideBySideCell>,
    deleted: bool,
    palette: &Palette,
) {
    let Some(cell) = cell else {
        buffer.set_style(area, Style::default().bg(palette.surface_dim));
        return;
    };
    let (fg, bg) = if cell.changed {
        if deleted {
            (palette.red, palette.surface0)
        } else {
            (palette.green, palette.surface0)
        }
    } else {
        (palette.text, palette.panel_bg)
    };
    buffer.set_style(area, Style::default().bg(bg));
    let number_width = 5u16.min(area.width);
    put_text(
        buffer,
        area.x,
        area.y,
        number_width,
        &format!("{:>4} ", cell.lineno),
        Style::default().fg(palette.overlay0).bg(bg),
    );
    put_text(
        buffer,
        area.x.saturating_add(number_width),
        area.y,
        area.width.saturating_sub(number_width),
        &cell.text,
        Style::default().fg(fg).bg(bg),
    );
}

pub(super) fn render_git_diff_overlay(
    b: &mut Buffer,
    overlay: &ClientGitDiffOverlay,
    p: &Palette,
) -> Option<OverlayRender> {
    let outer = popup(b.area, b.area.width, b.area.height)?;
    let inner = panel(b, outer, p.accent, p.panel_bg)?;
    if inner.width < 10 || inner.height < 4 {
        return None;
    }
    let mode = if overlay.staged { "staged" } else { "worktree" };
    put_text(
        b,
        inner.x,
        inner.y,
        inner.width,
        &format!(" {} ({mode})", overlay.path),
        Style::default()
            .fg(p.text)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    let close = Rect::new(inner.right().saturating_sub(11), inner.y, 11, 1);
    button(
        b,
        close,
        " esc close ",
        Style::default().fg(contrast(p)).bg(p.accent),
    );

    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(2),
    );
    if body.is_empty() {
        return Some(OverlayRender::default());
    }
    if overlay.loading {
        put_text(
            b,
            body.x,
            body.y,
            body.width,
            " loading…",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        return Some(OverlayRender::default());
    }
    if let Some(error) = &overlay.error {
        put_text(
            b,
            body.x,
            body.y,
            body.width,
            &format!(" {error}"),
            Style::default().fg(p.red).bg(p.panel_bg),
        );
        return Some(OverlayRender::default());
    }
    let Some(diff) = overlay.diff.as_ref() else {
        return Some(OverlayRender::default());
    };
    if diff.binary {
        put_text(
            b,
            body.x,
            body.y,
            body.width,
            " binary file, no preview",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        return Some(OverlayRender::default());
    }
    let rows = all_rows(diff);
    if rows.is_empty() {
        put_text(
            b,
            body.x,
            body.y,
            body.width,
            " no visible changes",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        return Some(OverlayRender::default());
    }

    let half = body.width / 2;
    let max_scroll = rows.len().saturating_sub(1);
    for (row_offset, row) in rows
        .iter()
        .skip(overlay.scroll.min(max_scroll))
        .take(body.height as usize)
        .enumerate()
    {
        let y = body.y + row_offset as u16;
        let left_area = Rect::new(body.x, y, half, 1);
        let right_area = Rect::new(body.x + half, y, body.width - half, 1);
        render_side_by_side_cell(b, left_area, row.left.as_ref(), true, p);
        render_side_by_side_cell(b, right_area, row.right.as_ref(), false, p);
    }

    Some(OverlayRender::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{GitDiffHunk, GitDiffLine, GitDiffLineKind, GitFileDiff};
    use crate::config::Config;

    fn test_palette() -> Palette {
        crate::app::client_palette_from_config(&Config::default())
    }

    fn buffer_text(buffer: &Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn line(kind: GitDiffLineKind, old: Option<u32>, new: Option<u32>, text: &str) -> GitDiffLine {
        GitDiffLine {
            kind,
            old_lineno: old,
            new_lineno: new,
            text: text.into(),
        }
    }

    #[test]
    fn side_by_side_pairs_equal_length_blocks() {
        let hunk = GitDiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![
                line(GitDiffLineKind::Deletion, Some(1), None, "old1"),
                line(GitDiffLineKind::Addition, None, Some(1), "new1"),
            ],
        };
        let rows = hunk_to_side_by_side(&hunk);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].left.as_ref().unwrap().text, "old1");
        assert_eq!(rows[0].right.as_ref().unwrap().text, "new1");
    }

    #[test]
    fn side_by_side_pads_shorter_side_with_blank_cells() {
        let hunk = GitDiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![
                line(GitDiffLineKind::Deletion, Some(1), None, "old1"),
                line(GitDiffLineKind::Deletion, Some(2), None, "old2"),
                line(GitDiffLineKind::Addition, None, Some(1), "new1"),
            ],
        };
        let rows = hunk_to_side_by_side(&hunk);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().unwrap().text, "old1");
        assert_eq!(rows[0].right.as_ref().unwrap().text, "new1");
        assert_eq!(rows[1].left.as_ref().unwrap().text, "old2");
        assert!(rows[1].right.is_none());
    }

    #[test]
    fn side_by_side_keeps_context_rows_unchanged() {
        let hunk = GitDiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![line(GitDiffLineKind::Context, Some(1), Some(1), "same")],
        };
        let rows = hunk_to_side_by_side(&hunk);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].left.as_ref().unwrap().changed);
        assert!(!rows[0].right.as_ref().unwrap().changed);
    }

    fn overlay(
        diff: Option<GitFileDiff>,
        loading: bool,
        error: Option<&str>,
    ) -> ClientGitDiffOverlay {
        ClientGitDiffOverlay {
            path: "f.rs".into(),
            staged: false,
            diff,
            scroll: 0,
            loading,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn loading_state_shows_a_placeholder() {
        let palette = test_palette();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
        render_git_diff_overlay(&mut buffer, &overlay(None, true, None), &palette);
        assert!(buffer_text(&buffer).contains("loading"));
    }

    #[test]
    fn error_state_shows_the_message() {
        let palette = test_palette();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
        render_git_diff_overlay(&mut buffer, &overlay(None, false, Some("boom")), &palette);
        assert!(buffer_text(&buffer).contains("boom"));
    }

    #[test]
    fn binary_diff_shows_a_placeholder_instead_of_content() {
        let palette = test_palette();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
        let diff = GitFileDiff {
            hunks: Vec::new(),
            binary: true,
        };
        render_git_diff_overlay(&mut buffer, &overlay(Some(diff), false, None), &palette);
        assert!(buffer_text(&buffer).contains("binary"));
    }

    #[test]
    fn renders_diff_line_text_and_line_numbers() {
        let palette = test_palette();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
        let diff = GitFileDiff {
            hunks: vec![GitDiffHunk {
                old_start: 1,
                new_start: 1,
                lines: vec![
                    line(GitDiffLineKind::Deletion, Some(5), None, "removed line"),
                    line(GitDiffLineKind::Addition, None, Some(5), "added line"),
                ],
            }],
            binary: false,
        };
        render_git_diff_overlay(&mut buffer, &overlay(Some(diff), false, None), &palette);
        let text = buffer_text(&buffer);
        assert!(text.contains("removed line"));
        assert!(text.contains("added line"));
        assert!(text.contains("f.rs"));
    }
}
