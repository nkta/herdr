use super::*;
use ratatui::style::Color;

/// Rows reserved for the wrapped message text below the commit box header, VS Code's Source
/// Control input being the reference point: a real multi-line field showing the whole draft,
/// not a single-line preview.
const COMMIT_BOX_MESSAGE_ROWS: u16 = 5;

fn status_glyph(
    status: crate::protocol::ClientShellGitFileStatus,
    palette: &Palette,
) -> (&'static str, Color) {
    use crate::protocol::ClientShellGitFileStatus as Status;
    match status {
        Status::Added => ("A", palette.green),
        Status::Modified => ("M", palette.yellow),
        Status::Deleted => ("D", palette.red),
        Status::Renamed => ("R", palette.teal),
        Status::Untracked => ("?", palette.blue),
        Status::Conflicted => ("U", palette.red),
        Status::Unknown => ("?", palette.overlay0),
    }
}

enum GitPanelLine<'a> {
    SectionHeader(bool),
    File {
        index: usize,
        entry: &'a crate::protocol::ClientShellGitFileEntry,
    },
}

/// Flattened visual line list, in the panel's rendering order: a "STAGED CHANGES" header
/// followed by staged entries, then a "CHANGES" header followed by unstaged/untracked entries.
/// `index` on `File` lines is a plain row counter shared by rendering and (in a later commit)
/// input hit-testing, so both agree on which selectable row is which without recomputing it.
fn panel_lines(working_tree: &crate::protocol::ClientShellGitWorkingTree) -> Vec<GitPanelLine<'_>> {
    let mut lines = Vec::new();
    let mut index = 0;
    if !working_tree.staged.is_empty() {
        lines.push(GitPanelLine::SectionHeader(true));
        for entry in &working_tree.staged {
            lines.push(GitPanelLine::File { index, entry });
            index += 1;
        }
    }
    if !working_tree.unstaged.is_empty() {
        lines.push(GitPanelLine::SectionHeader(false));
        for entry in &working_tree.unstaged {
            lines.push(GitPanelLine::File { index, entry });
            index += 1;
        }
    }
    lines
}

/// Renders the sidebar's Git panel for the focused workspace. Returns the hit-test rect for
/// each rendered file row keyed by its index into `panel_lines`'s `File` entries, plus the
/// commit box's own hit-test rect (empty when the box wasn't drawn, e.g. no room or no repo).
pub(super) fn render_git_panel(
    buffer: &mut Buffer,
    area: Rect,
    workspace: Option<&crate::protocol::ClientShellWorkspace>,
    git_panel: &ClientGitPanelState,
    palette: &Palette,
) -> (Vec<(Rect, usize)>, Rect) {
    if area.is_empty() {
        return (Vec::new(), Rect::default());
    }
    let Some(workspace) = workspace else {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            " no focused workspace",
            Style::default().fg(palette.overlay0),
        );
        return (Vec::new(), Rect::default());
    };
    if !workspace.git_repo {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            " not a git repository",
            Style::default().fg(palette.overlay0),
        );
        return (Vec::new(), Rect::default());
    }

    let mut y = area.y;
    let branch = workspace.branch.as_deref().unwrap_or("(detached)");
    let ahead_behind = workspace
        .git_ahead_behind
        .map(|(ahead, behind)| format!(" ↑{ahead} ↓{behind}"))
        .unwrap_or_default();
    put_text(
        buffer,
        area.x,
        y,
        area.width,
        &format!(" {branch}{ahead_behind}"),
        Style::default()
            .fg(palette.mauve)
            .add_modifier(Modifier::BOLD),
    );
    y = y.saturating_add(1);
    if y >= area.bottom() {
        return (Vec::new(), Rect::default());
    }

    if let Some(path) = &git_panel.pending_discard {
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            &format!(" discard {path}? (y/n)"),
            Style::default()
                .fg(palette.red)
                .add_modifier(Modifier::BOLD),
        );
        y = y.saturating_add(1);
    } else if let Some(error) = &git_panel.last_error {
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            &format!(" {error}"),
            Style::default().fg(palette.red),
        );
        y = y.saturating_add(1);
    }
    if y >= area.bottom() {
        return (Vec::new(), Rect::default());
    }

    // The commit box is always a real multi-row text field — not just when the draft is long —
    // and always claims its rows at the bottom, so the file list shrinks first.
    let wrap_width = area.width.saturating_sub(1);
    let wrapped_message = wrap_commit_message(&git_panel.commit_message, wrap_width);
    let desired_commit_box_height = COMMIT_BOX_MESSAGE_ROWS.saturating_add(1);
    let commit_box_top = area
        .bottom()
        .saturating_sub(desired_commit_box_height)
        .max(y);
    let list_bottom = commit_box_top;
    render_commit_box(
        buffer,
        area,
        commit_box_top,
        git_panel,
        &wrapped_message,
        palette,
    );
    let commit_box_height = area
        .bottom()
        .saturating_sub(commit_box_top)
        .min(desired_commit_box_height);
    let commit_box_rect = if commit_box_height == 0 {
        Rect::default()
    } else {
        Rect::new(area.x, commit_box_top, area.width, commit_box_height)
    };

    let Some(working_tree) = workspace.git_working_tree.as_ref() else {
        if y < list_bottom {
            put_text(
                buffer,
                area.x,
                y,
                area.width,
                " loading…",
                Style::default().fg(palette.overlay0),
            );
        }
        return (Vec::new(), commit_box_rect);
    };

    let lines = panel_lines(working_tree);
    if lines.is_empty() {
        if y < list_bottom {
            put_text(
                buffer,
                area.x,
                y,
                area.width,
                " no changes",
                Style::default().fg(palette.overlay0),
            );
        }
        return (Vec::new(), commit_box_rect);
    }

    let max_scroll = lines.len().saturating_sub(1);
    let mut hits = Vec::new();
    for line in lines.iter().skip(git_panel.scroll.min(max_scroll)) {
        if y >= list_bottom {
            break;
        }
        match line {
            GitPanelLine::SectionHeader(staged) => {
                put_text(
                    buffer,
                    area.x,
                    y,
                    area.width,
                    if *staged {
                        " STAGED CHANGES"
                    } else {
                        " CHANGES"
                    },
                    Style::default().fg(palette.overlay0),
                );
            }
            GitPanelLine::File { index, entry } => {
                let rect = Rect::new(area.x, y, area.width, 1);
                if *index == git_panel.selected {
                    buffer.set_style(rect, Style::default().bg(palette.selection_bg));
                }
                let (glyph, glyph_color) = status_glyph(entry.status, palette);
                put_text(
                    buffer,
                    rect.x,
                    rect.y,
                    rect.width.saturating_sub(2),
                    &format!(" {}", entry.path),
                    Style::default().fg(palette.text),
                );
                put_text(
                    buffer,
                    rect.right().saturating_sub(2),
                    rect.y,
                    2,
                    &format!(" {glyph}"),
                    Style::default().fg(glyph_color),
                );
                hits.push((rect, *index));
            }
        }
        y = y.saturating_add(1);
    }
    (hits, commit_box_rect)
}

/// Renders the commit box pinned to the bottom of the panel: a header naming the keybinding to
/// submit, and a real multi-line, word-wrapped text field for the draft message below it —
/// modeled on VS Code's Source Control input rather than a single-line preview. Editing only
/// ever appends at (or backspaces from) the end of the message, so the field is bottom-anchored:
/// it always shows the tail of the wrapped text, keeping the insertion point in view.
fn render_commit_box(
    buffer: &mut Buffer,
    area: Rect,
    top: u16,
    git_panel: &ClientGitPanelState,
    wrapped: &[String],
    palette: &Palette,
) {
    if top >= area.bottom() {
        return;
    }
    let focused = git_panel.focus == GitSidebarFocus::CommitBox;
    let header = if git_panel.commit_in_flight {
        " committing…"
    } else if git_panel.generating_commit_message {
        " generating…"
    } else {
        " COMMIT MESSAGE"
    };
    put_text(
        buffer,
        area.x,
        top,
        area.width,
        header,
        Style::default().fg(if focused {
            palette.text
        } else {
            palette.overlay0
        }),
    );

    let message_top = top.saturating_add(1);
    if message_top >= area.bottom() {
        return;
    }
    let message_height = area.bottom() - message_top;
    let box_bg = if focused {
        palette.surface0
    } else {
        palette.panel_bg
    };
    let message_area = Rect::new(area.x, message_top, area.width, message_height);
    buffer.set_style(message_area, Style::default().bg(box_bg));

    let is_empty = git_panel.commit_message.is_empty();
    let visible_height = usize::from(message_height);
    let wrap_width = area.width.saturating_sub(1);
    let (cursor_row, cursor_column) = commit_cursor_position(
        &git_panel.commit_message,
        git_panel.commit_cursor,
        wrap_width,
    );
    // Bottom-anchored by default (show the tail, matching the common "editing at the end" case),
    // but scrolled up just enough to keep the cursor in view when it's earlier in the draft.
    let first_visible = wrapped.len().saturating_sub(visible_height).min(cursor_row);
    let text_style = Style::default()
        .fg(if is_empty {
            palette.overlay0
        } else {
            palette.text
        })
        .bg(box_bg);

    for (row_offset, line) in wrapped[first_visible..]
        .iter()
        .enumerate()
        .take(visible_height)
    {
        let y = message_top + row_offset as u16;
        if is_empty && row_offset == 0 {
            put_text(
                buffer,
                area.x + 1,
                y,
                area.width.saturating_sub(1),
                "Message…",
                text_style,
            );
        } else {
            put_text(
                buffer,
                area.x + 1,
                y,
                area.width.saturating_sub(1),
                line,
                text_style,
            );
        }
        if focused && first_visible + row_offset == cursor_row {
            let cursor_x = area.x + 1 + cursor_column;
            if cursor_x < area.right() {
                buffer.set_style(
                    Rect::new(cursor_x, y, 1, 1),
                    Style::default().fg(box_bg).bg(palette.text),
                );
            }
        }
    }
}

/// The wrapped-row index and display column of `cursor` (a byte offset into the un-wrapped
/// commit message, always on a char boundary) within `wrap_commit_message`'s output for the same
/// text and width. Computed by reusing that same wrap function on the preceding logical lines and
/// on the current line's prefix up to the cursor, so the row/column always agrees with what's
/// actually rendered instead of tracking offsets down a separate path that could drift.
fn commit_cursor_position(commit_message: &str, cursor: usize, wrap_width: u16) -> (usize, u16) {
    let line_start = commit_message[..cursor]
        .rfind('\n')
        .map_or(0, |pos| pos + 1);
    let preceding_paragraphs = commit_message[..line_start].matches('\n').count();
    let preceding_rows: usize = commit_message
        .split('\n')
        .take(preceding_paragraphs)
        .map(|paragraph| wrap_commit_message(paragraph, wrap_width).len())
        .sum();
    let partial = wrap_commit_message(&commit_message[line_start..cursor], wrap_width);
    let row = preceding_rows + partial.len() - 1;
    let column = display_width(partial.last().map_or("", String::as_str));
    (row, column)
}

/// Word-wraps `text` to `width` columns for display only — the stored draft keeps its original
/// spacing and newlines, only the on-screen rendering wraps. Each manual newline starts a new
/// paragraph; words within a paragraph are packed greedily, and a single word longer than
/// `width` is placed alone on its line (display clipping trims it rather than a manual
/// character-level break). Always returns at least one (possibly empty) line.
fn wrap_commit_message(text: &str, width: u16) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let candidate_width = if current.is_empty() {
                display_width(word)
            } else {
                display_width(&current) + 1 + display_width(word)
            };
            if current.is_empty() || candidate_width <= width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::protocol::{
        ClientShellGitFileEntry, ClientShellGitFileStatus, ClientShellGitWorkingTree,
        ClientShellWorkspace,
    };

    fn test_palette() -> Palette {
        crate::app::client_palette_from_config(&Config::default())
    }

    fn buffer_text(buffer: &Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn workspace(
        git_repo: bool,
        git_working_tree: Option<ClientShellGitWorkingTree>,
    ) -> ClientShellWorkspace {
        ClientShellWorkspace {
            workspace_id: "w1".into(),
            active_tab_id: "w1:t1".into(),
            new_workspace_cwd: "/repo".into(),
            number: 1,
            label: "repo".into(),
            custom_label: false,
            branch: Some("main".into()),
            git_ahead_behind: Some((2, 1)),
            git_repo,
            git_working_tree,
            tokens: Vec::new(),
            worktree: None,
            focused: true,
            agent_status: crate::api::schema::AgentStatus::Idle,
        }
    }

    #[test]
    fn no_focused_workspace_shows_placeholder() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let git_panel = ClientGitPanelState::default();

        let (hits, _commit_box) = render_git_panel(&mut buffer, area, None, &git_panel, &palette);

        assert!(hits.is_empty());
        assert!(buffer_text(&buffer).contains("no focused workspace"));
    }

    #[test]
    fn non_git_workspace_shows_placeholder() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let git_panel = ClientGitPanelState::default();
        let ws = workspace(false, None);

        let (hits, _commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        assert!(hits.is_empty());
        assert!(buffer_text(&buffer).contains("not a git repository"));
    }

    #[test]
    fn pending_demand_shows_loading() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let git_panel = ClientGitPanelState::default();
        let ws = workspace(true, None);

        let (hits, _commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        assert!(hits.is_empty());
        let text = buffer_text(&buffer);
        assert!(text.contains("main"));
        assert!(text.contains("loading"));
    }

    #[test]
    fn empty_working_tree_shows_no_changes() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let git_panel = ClientGitPanelState::default();
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));

        let (hits, _commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        assert!(hits.is_empty());
        assert!(buffer_text(&buffer).contains("no changes"));
    }

    #[test]
    fn renders_branch_ahead_behind_and_file_rows() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 16);
        let mut buffer = Buffer::empty(area);
        let git_panel = ClientGitPanelState::default();
        let working_tree = ClientShellGitWorkingTree {
            staged: vec![ClientShellGitFileEntry {
                path: "staged.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Added,
            }],
            unstaged: vec![ClientShellGitFileEntry {
                path: "unstaged.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Modified,
            }],
        };
        let ws = workspace(true, Some(working_tree));

        let (hits, _commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let text = buffer_text(&buffer);
        assert!(text.contains("main"));
        assert!(text.contains("↑2"));
        assert!(text.contains("↓1"));
        assert!(text.contains("STAGED CHANGES"));
        assert!(text.contains("staged.rs"));
        assert!(text.contains("CHANGES"));
        assert!(text.contains("unstaged.rs"));
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].1, 0);
        assert_eq!(hits[1].1, 1);
        assert!(hits[0].0.y < hits[1].0.y);
    }

    #[test]
    fn commit_box_hit_rect_sits_below_the_file_rows() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let git_panel = ClientGitPanelState::default();
        let working_tree = ClientShellGitWorkingTree {
            staged: vec![ClientShellGitFileEntry {
                path: "staged.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Added,
            }],
            unstaged: Vec::new(),
        };
        let ws = workspace(true, Some(working_tree));

        let (hits, commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        assert_eq!(commit_box.height, COMMIT_BOX_MESSAGE_ROWS + 1);
        assert_eq!(commit_box.y, area.bottom() - (COMMIT_BOX_MESSAGE_ROWS + 1));
        assert!(hits.iter().all(|(rect, _)| rect.y < commit_box.y));
    }

    #[test]
    fn scroll_skips_leading_lines() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let working_tree = ClientShellGitWorkingTree {
            staged: vec![ClientShellGitFileEntry {
                path: "staged.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Added,
            }],
            unstaged: Vec::new(),
        };
        let ws = workspace(true, Some(working_tree));

        let mut buffer_no_scroll = Buffer::empty(area);
        let no_scroll = ClientGitPanelState::default();
        render_git_panel(&mut buffer_no_scroll, area, Some(&ws), &no_scroll, &palette);
        assert!(buffer_text(&buffer_no_scroll).contains("STAGED CHANGES"));

        let mut buffer_scrolled = Buffer::empty(area);
        let scrolled = ClientGitPanelState {
            selected: 0,
            scroll: 1,
            ..Default::default()
        };
        let (hits, _commit_box) =
            render_git_panel(&mut buffer_scrolled, area, Some(&ws), &scrolled, &palette);
        let text = buffer_text(&buffer_scrolled);
        assert!(!text.contains("STAGED CHANGES"));
        assert!(text.contains("staged.rs"));
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn commit_box_shows_the_full_wrapped_message_not_just_the_first_line() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 15);
        let mut buffer = Buffer::empty(area);
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));
        let git_panel = ClientGitPanelState {
            commit_message: "first line\nsecond line\nthird line".into(),
            ..Default::default()
        };

        render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let text = buffer_text(&buffer);
        assert!(text.contains("first line"));
        assert!(text.contains("second line"));
        assert!(text.contains("third line"));
        assert!(!text.contains("(+2)"));
    }

    #[test]
    fn commit_box_height_is_fixed_regardless_of_message_length() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 20);
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));

        let mut buffer_short = Buffer::empty(area);
        let short = ClientGitPanelState {
            commit_message: "short".into(),
            ..Default::default()
        };
        let (_, short_box) = render_git_panel(&mut buffer_short, area, Some(&ws), &short, &palette);

        let mut buffer_long = Buffer::empty(area);
        let long = ClientGitPanelState {
            commit_message: "one two three four five six seven eight nine ten eleven twelve \
                              thirteen fourteen fifteen sixteen"
                .into(),
            ..Default::default()
        };
        let (_, long_box) = render_git_panel(&mut buffer_long, area, Some(&ws), &long, &palette);

        assert_eq!(short_box.height, COMMIT_BOX_MESSAGE_ROWS + 1);
        assert_eq!(long_box.height, COMMIT_BOX_MESSAGE_ROWS + 1);
    }

    #[test]
    fn commit_box_scrolls_to_show_the_tail_of_a_message_taller_than_the_box() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 20);
        let mut buffer = Buffer::empty(area);
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));
        let message = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight";
        let git_panel = ClientGitPanelState {
            commit_message: message.into(),
            commit_cursor: message.len(),
            ..Default::default()
        };

        render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let text = buffer_text(&buffer);
        assert!(!text.contains("one"));
        assert!(!text.contains("two"));
        assert!(!text.contains("three"));
        assert!(text.contains("four"));
        assert!(text.contains("eight"));
    }

    #[test]
    fn empty_commit_box_shows_placeholder_text() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));
        let git_panel = ClientGitPanelState::default();

        render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        assert!(buffer_text(&buffer).contains("Message…"));
    }

    #[test]
    fn focused_commit_box_draws_a_cursor_after_the_text() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));
        let git_panel = ClientGitPanelState {
            commit_message: "hi".into(),
            commit_cursor: 2,
            focus: GitSidebarFocus::CommitBox,
            ..Default::default()
        };

        let (_, commit_box) = render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let message_y = commit_box.y + 1;
        let cursor_x = area.x + 1 + display_width("hi");
        assert_eq!(buffer[(cursor_x, message_y)].bg, palette.text);
    }

    #[test]
    fn commit_box_scrolls_up_to_reveal_a_cursor_earlier_in_a_long_message() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 20);
        let mut buffer = Buffer::empty(area);
        let ws = workspace(true, Some(ClientShellGitWorkingTree::default()));
        let message = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight";
        let git_panel = ClientGitPanelState {
            commit_message: message.into(),
            commit_cursor: 0, // at the very start, on "one"
            focus: GitSidebarFocus::CommitBox,
            ..Default::default()
        };

        render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let text = buffer_text(&buffer);
        assert!(text.contains("one"));
        assert!(text.contains("two"));
        assert!(!text.contains("eight"));
    }

    #[test]
    fn commit_cursor_position_locates_the_wrapped_row_and_column() {
        let message = "fix bug\nlonger body line";
        // Cursor right after "fix bug" (end of the first logical line).
        let (row, column) = commit_cursor_position(message, 7, 30);
        assert_eq!(row, 0);
        assert_eq!(column, display_width("fix bug"));

        // Cursor at the very start of the second line.
        let (row, column) = commit_cursor_position(message, 8, 30);
        assert_eq!(row, 1);
        assert_eq!(column, 0);

        // Cursor mid-word wraps to a second row when the width forces a break.
        let (row, column) = commit_cursor_position("one two three", 13, 7);
        assert_eq!(row, 1);
        assert_eq!(column, display_width("three"));
    }

    #[test]
    fn wrap_commit_message_packs_words_and_keeps_manual_blank_lines() {
        let wrapped = wrap_commit_message("fix bug\n\nlonger explanation across two lines", 12);
        assert_eq!(wrapped[0], "fix bug");
        assert_eq!(wrapped[1], "");
        assert!(wrapped[2..].iter().all(|line| display_width(line) <= 12));
        assert_eq!(
            wrapped[2..].join(" "),
            "longer explanation across two lines"
        );
    }

    #[test]
    fn wrap_commit_message_of_empty_text_returns_one_empty_line() {
        assert_eq!(wrap_commit_message("", 10), vec![String::new()]);
    }

    #[test]
    fn file_row_shows_the_name_first_and_the_status_glyph_at_the_right_edge() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let working_tree = ClientShellGitWorkingTree {
            staged: vec![ClientShellGitFileEntry {
                path: "staged.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Added,
            }],
            unstaged: Vec::new(),
        };
        let ws = workspace(true, Some(working_tree));
        let git_panel = ClientGitPanelState::default();

        let (hits, _commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let file_row_y = hits[0].0.y;
        assert_eq!(buffer[(area.x + 1, file_row_y)].symbol(), "s");
        assert_eq!(buffer[(area.right() - 1, file_row_y)].symbol(), "A");
    }

    #[test]
    fn selected_row_gets_highlighted_background() {
        let palette = test_palette();
        let area = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(area);
        let working_tree = ClientShellGitWorkingTree {
            staged: vec![
                ClientShellGitFileEntry {
                    path: "a.rs".into(),
                    original_path: None,
                    status: ClientShellGitFileStatus::Added,
                },
                ClientShellGitFileEntry {
                    path: "b.rs".into(),
                    original_path: None,
                    status: ClientShellGitFileStatus::Added,
                },
            ],
            unstaged: Vec::new(),
        };
        let ws = workspace(true, Some(working_tree));
        let git_panel = ClientGitPanelState {
            selected: 1,
            ..Default::default()
        };

        let (hits, _commit_box) =
            render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        let selected_rect = hits.iter().find(|(_, index)| *index == 1).unwrap().0;
        let other_rect = hits.iter().find(|(_, index)| *index == 0).unwrap().0;
        assert_eq!(
            buffer[(selected_rect.x, selected_rect.y)].bg,
            palette.selection_bg
        );
        assert_ne!(
            buffer[(other_rect.x, other_rect.y)].bg,
            palette.selection_bg
        );
    }
}
