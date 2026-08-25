use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::app::state::{GitFileRowArea, Palette, SidebarSpacesView};
use crate::app::AppState;
use crate::workspace::{GitFileEntry, GitFileStatusKind};

use super::widgets::render_panel_shell;

pub(crate) const GIT_PANEL_BRANCH_ROWS: u16 = 1;
pub(crate) const GIT_PANEL_COMMIT_BOX_HEIGHT: u16 = 5;
const GIT_PANEL_SECTION_HEADER_ROWS: u16 = 1;

/// Tab labels for the sidebar's top section, in render order. Shared by the `Tabs` render in
/// `sidebar.rs` and the hit-test geometry below so the two can never drift apart.
pub(crate) const SIDEBAR_TAB_LABELS: [(SidebarSpacesView, &str); 2] = [
    (SidebarSpacesView::Spaces, "spaces"),
    (SidebarSpacesView::Git, "git"),
];

/// Hit-test rects for the sidebar tab strip.
///
/// These mirror the exact column layout `Tabs::new(..).divider(" ").padding(" ", " ")` produces —
/// each tab occupies `pad_left + label + pad_right`, separated by a one-column divider. Splitting
/// the header row evenly instead would leave the clickable area offset from the drawn label.
pub(crate) fn sidebar_tab_hit_areas(area: Rect) -> Vec<(SidebarSpacesView, Rect)> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }

    let row_end = area.x.saturating_add(area.width);
    let mut hit_areas = Vec::with_capacity(SIDEBAR_TAB_LABELS.len());
    let mut x = area.x;

    for (index, (view, label)) in SIDEBAR_TAB_LABELS.iter().enumerate() {
        if index > 0 {
            // divider column between tabs
            x = x.saturating_add(1);
        }
        // one padding column on each side of the label
        let cell_width = super::text::display_width_u16(label).saturating_add(2);
        if x >= row_end {
            break;
        }
        let width = cell_width.min(row_end - x);
        hit_areas.push((*view, Rect::new(x, area.y, width, 1)));
        x = x.saturating_add(cell_width);
    }

    hit_areas
}

/// Commit message box rect and the "commit" submit button rect within it (a click target kept
/// alongside `Ctrl+Enter` since not every terminal can report it — see `sidebar_git.rs`).
pub(crate) fn compute_git_commit_box_rects(area: Rect) -> (Rect, Rect) {
    if area.height <= GIT_PANEL_BRANCH_ROWS || area.width == 0 {
        return (Rect::default(), Rect::default());
    }
    let box_height = GIT_PANEL_COMMIT_BOX_HEIGHT.min(area.height - GIT_PANEL_BRANCH_ROWS);
    let box_rect = Rect::new(
        area.x,
        area.y + GIT_PANEL_BRANCH_ROWS,
        area.width,
        box_height,
    );
    if box_rect.height < 3 {
        return (box_rect, Rect::default());
    }
    let submit_width = 8u16.min(box_rect.width.saturating_sub(2));
    let submit_rect = Rect::new(
        box_rect.x + box_rect.width.saturating_sub(submit_width + 1),
        box_rect.y + box_rect.height - 2,
        submit_width,
        1,
    );
    (box_rect, submit_rect)
}

/// Hit-test rects for each staged/unstaged file row, in the sidebar's visual order (staged
/// first). Shared by `compute_view_internal` (populates `ViewState`) and `render_git_panel`
/// (reads the same rects back rather than recomputing row positions).
pub(crate) fn compute_git_file_row_areas(app: &AppState, area: Rect) -> Vec<GitFileRowArea> {
    let mut rows = Vec::new();
    let list_top = area.y + GIT_PANEL_BRANCH_ROWS + GIT_PANEL_COMMIT_BOX_HEIGHT;
    let bottom = area.y + area.height;
    if list_top >= bottom {
        return rows;
    }

    let mut y = list_top + GIT_PANEL_SECTION_HEADER_ROWS;
    let empty = Vec::new();
    let staged = app
        .git_sidebar
        .status
        .as_ref()
        .map(|s| &s.staged)
        .unwrap_or(&empty);
    let unstaged = app
        .git_sidebar
        .status
        .as_ref()
        .map(|s| &s.unstaged)
        .unwrap_or(&empty);

    // Rows are emitted from the scroll offset onward, so a list longer than the panel stays
    // reachable. Section headers are only drawn when their section still has a visible row.
    let scroll = app.git_sidebar.scroll;
    let staged_visible = staged.len().saturating_sub(scroll);

    for entry in staged.iter().skip(scroll) {
        if y >= bottom {
            return rows;
        }
        rows.push(git_file_row_area(area, y, entry, true));
        y += 1;
    }

    let unstaged_skip = scroll.saturating_sub(staged.len());
    if staged_visible > 0 || unstaged_skip == 0 {
        y += GIT_PANEL_SECTION_HEADER_ROWS;
    }
    for entry in unstaged.iter().skip(unstaged_skip) {
        if y >= bottom {
            return rows;
        }
        rows.push(git_file_row_area(area, y, entry, false));
        y += 1;
    }

    rows
}

/// How many file rows fit in the panel, used to clamp scrolling and to keep the selected row on
/// screen.
pub(crate) fn git_file_list_capacity(area: Rect) -> usize {
    let list_top = area.y + GIT_PANEL_BRANCH_ROWS + GIT_PANEL_COMMIT_BOX_HEIGHT;
    let bottom = area.y + area.height;
    // Two section headers share the space with the rows.
    (bottom.saturating_sub(list_top) as usize)
        .saturating_sub(2 * GIT_PANEL_SECTION_HEADER_ROWS as usize)
}

/// Width of one action button (`+`/`-`/`x`) at the right edge of a file row: the glyph plus a
/// column of breathing room on each side, so the click target is comfortable.
const GIT_ROW_ACTION_WIDTH: u16 = 3;

fn git_file_row_area(area: Rect, y: u16, entry: &GitFileEntry, staged: bool) -> GitFileRowArea {
    let icon_width = GIT_ROW_ACTION_WIDTH.min(area.width);
    let can_discard = !staged && entry.status != GitFileStatusKind::Untracked;
    GitFileRowArea {
        row_rect: Rect::new(area.x, y, area.width, 1),
        stage_toggle_rect: Rect::new(
            area.x + area.width.saturating_sub(icon_width * 2),
            y,
            icon_width,
            1,
        ),
        // Collapse the target to nothing when discard does not apply, so a stray click in that
        // column cannot trigger an action the row does not offer.
        discard_rect: if can_discard {
            Rect::new(
                area.x + area.width.saturating_sub(icon_width),
                y,
                icon_width,
                1,
            )
        } else {
            Rect::default()
        },
        path: entry.path.clone(),
        staged,
        can_discard,
    }
}

fn status_glyph_and_color(kind: GitFileStatusKind, p: &Palette) -> (char, Color) {
    match kind {
        GitFileStatusKind::Modified => ('M', p.yellow),
        GitFileStatusKind::Added => ('A', p.green),
        GitFileStatusKind::Deleted => ('D', p.red),
        GitFileStatusKind::Renamed => ('R', p.mauve),
        GitFileStatusKind::Untracked => ('U', p.overlay0),
        GitFileStatusKind::Conflicted => ('!', p.red),
    }
}

pub(super) fn render_git_panel(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let p = &app.palette;

    render_branch_header(
        app,
        frame,
        Rect::new(area.x, area.y, area.width, GIT_PANEL_BRANCH_ROWS),
    );
    render_commit_box(
        app,
        frame,
        app.view.git_commit_box_rect,
        app.view.git_commit_submit_hit_area,
    );

    let list_top = area.y + GIT_PANEL_BRANCH_ROWS + GIT_PANEL_COMMIT_BOX_HEIGHT;
    let bottom = area.y + area.height;
    if list_top >= bottom {
        return;
    }

    let status = app.git_sidebar.status.as_ref();
    let staged_count = status.map(|s| s.staged.len()).unwrap_or(0);
    let unstaged_count = status.map(|s| s.unstaged.len()).unwrap_or(0);
    let selected = app
        .git_sidebar
        .selected_row()
        .map(|(entry, staged)| (entry.path.clone(), staged));

    // Header positions are derived from the rendered rows so they stay correct while scrolled,
    // instead of being recomputed from the full (unscrolled) counts.
    let rows = &app.view.git_file_row_areas;
    let first_staged = rows.iter().find(|row| row.staged);
    let first_unstaged = rows.iter().find(|row| !row.staged);

    if let Some(row) = first_staged {
        let header_y = row.row_rect.y.saturating_sub(1);
        if header_y >= list_top {
            frame.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    format!(" staged changes ({staged_count})"),
                    Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
                )])),
                Rect::new(area.x, header_y, area.width, 1),
            );
        }
    }

    if let Some(row) = first_unstaged {
        let header_y = row.row_rect.y.saturating_sub(1);
        if header_y >= list_top {
            frame.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    format!(" changes ({unstaged_count})"),
                    Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
                )])),
                Rect::new(area.x, header_y, area.width, 1),
            );
        }
    }

    for row in rows {
        render_git_file_row(app, frame, row, selected.as_ref());
    }

    if staged_count == 0 && unstaged_count == 0 {
        let empty_y = list_top + GIT_PANEL_SECTION_HEADER_ROWS;
        if empty_y < bottom {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    " no changes",
                    Style::default().fg(p.overlay0).add_modifier(Modifier::DIM),
                )),
                Rect::new(area.x, empty_y, area.width, 1),
            );
        }
    }
}

fn render_branch_header(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let p = &app.palette;
    let ws = app.active.and_then(|idx| app.workspaces.get(idx));
    let mut spans = vec![Span::raw(" ")];
    match ws.and_then(|ws| ws.branch()) {
        Some(branch) => spans.push(Span::styled(branch, Style::default().fg(p.mauve))),
        None => spans.push(Span::styled("no branch", Style::default().fg(p.overlay0))),
    }
    if let Some((ahead, behind)) = ws.and_then(|ws| ws.git_ahead_behind()) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("\u{2193}{behind}"),
            Style::default().fg(p.red),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("\u{2191}{ahead}"),
            Style::default().fg(p.green),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_commit_box(app: &AppState, frame: &mut Frame, box_rect: Rect, submit_rect: Rect) {
    if box_rect.height == 0 || box_rect.width == 0 {
        return;
    }
    let p = &app.palette;
    let focused = app.mode == crate::app::Mode::SidebarGit
        && app.git_sidebar.focus == crate::app::GitSidebarFocus::CommitBox;
    let border_color = if focused { p.accent } else { p.surface_dim };
    let Some(inner) = render_panel_shell(frame, box_rect, border_color, p.sidebar_bg) else {
        return;
    };

    let placeholder = app.git_sidebar.commit_message.is_empty();
    let text = if placeholder {
        "commit message\u{2026}".to_string()
    } else {
        app.git_sidebar.commit_message.clone()
    };
    let text_style = if placeholder {
        Style::default().fg(p.overlay0).add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(p.text)
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(text_style)
            .wrap(Wrap { trim: false }),
        inner,
    );

    if submit_rect.height > 0 {
        let submit_style = if app.git_sidebar.commit_in_flight {
            Style::default().fg(p.overlay0).add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
        };
        let label = if app.git_sidebar.commit_in_flight {
            "committing\u{2026}"
        } else {
            "commit"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(label, submit_style)),
            submit_rect,
        );
    }
}

fn render_git_file_row(
    app: &AppState,
    frame: &mut Frame,
    row: &GitFileRowArea,
    selected: Option<&(String, bool)>,
) {
    let p = &app.palette;
    let is_selected =
        selected.is_some_and(|(path, staged)| path == &row.path && *staged == row.staged);
    let is_pending_discard = app.git_sidebar.pending_discard.as_deref() == Some(row.path.as_str());

    if is_selected {
        // Reuse the workspace list's selection background rather than `p.selection_bg` directly:
        // that helper falls back to `active_row_bg` for themes whose `selection_bg` is
        // `Color::Reset` (the "terminal" theme), where painting Reset would show no highlight.
        let bg = super::sidebar::workspace_selection_background(p, true);
        let buf = frame.buffer_mut();
        for x in row.row_rect.x..row.row_rect.x + row.row_rect.width {
            buf[(x, row.row_rect.y)].set_style(Style::default().bg(bg));
        }
    }

    let entries = row_source(app, row.staged);
    let Some(entry) = entries.iter().find(|entry| entry.path == row.path) else {
        return;
    };
    let (glyph, color) = status_glyph_and_color(entry.status, p);

    if is_pending_discard {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" discard changes to {}? (y/n)", entry.path),
                Style::default().fg(p.red).add_modifier(Modifier::BOLD),
            )),
            row.row_rect,
        );
        return;
    }

    let name_style = if is_selected {
        Style::default().fg(p.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.subtext0)
    };

    // Reserve the action columns so a long path can never render underneath the buttons.
    let actions_width = if row.staged {
        GIT_ROW_ACTION_WIDTH
    } else {
        GIT_ROW_ACTION_WIDTH * 2
    };
    let name_budget = row
        .row_rect
        .width
        .saturating_sub(3) // leading space + status glyph + space
        .saturating_sub(actions_width) as usize;
    let display_path = super::text::truncate_end(&entry.path, name_budget);

    let spans = vec![
        Span::raw(" "),
        Span::styled(
            glyph.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(display_path, name_style),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), row.row_rect);

    render_row_actions(frame, p, row, is_selected);
}

/// Draws the per-row action buttons. Herdr is a mouse-first TUI, so these click targets must be
/// visible rather than implied: staged rows offer unstage (`-`), unstaged rows offer stage (`+`)
/// and discard (`x`).
fn render_row_actions(frame: &mut Frame, p: &Palette, row: &GitFileRowArea, is_selected: bool) {
    // Keep the buttons legible on the selected row's highlight, and muted elsewhere so the file
    // list stays readable.
    let action_style = if is_selected {
        Style::default().fg(p.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.overlay0)
    };

    let toggle_glyph = if row.staged { "-" } else { "+" };
    if row.stage_toggle_rect.width > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(toggle_glyph, action_style)).alignment(Alignment::Center),
            row.stage_toggle_rect,
        );
    }

    if row.can_discard && row.discard_rect.width > 0 {
        let discard_style = if is_selected {
            Style::default().fg(p.red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay0)
        };
        frame.render_widget(
            Paragraph::new(Span::styled("x", discard_style)).alignment(Alignment::Center),
            row.discard_rect,
        );
    }
}

fn row_source(app: &AppState, staged: bool) -> &[GitFileEntry] {
    let empty: &[GitFileEntry] = &[];
    let Some(status) = app.git_sidebar.status.as_ref() else {
        return empty;
    };
    if staged {
        &status.staged
    } else {
        &status.unstaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clickable area of each tab must cover the columns its label is actually drawn on.
    /// A previous version split the header row in half, which left `git`'s hit area starting
    /// three columns to the right of the rendered label.
    #[test]
    fn tab_hit_areas_cover_the_columns_their_labels_render_on() {
        let area = Rect::new(0, 0, 26, 1);
        let hits = sidebar_tab_hit_areas(area);
        assert_eq!(hits.len(), 2);

        // Layout mirrors `Tabs.divider(" ").padding(" ", " ")`:
        // " spaces " | " " | " git "  ->  labels at x=1..=6 and x=10..=12.
        let (spaces_view, spaces_rect) = hits[0];
        let (git_view, git_rect) = hits[1];
        assert_eq!(spaces_view, SidebarSpacesView::Spaces);
        assert_eq!(git_view, SidebarSpacesView::Git);

        let covers = |rect: Rect, x: u16| x >= rect.x && x < rect.x + rect.width;
        for x in 1..=6 {
            assert!(
                covers(spaces_rect, x),
                "spaces label column {x} not covered"
            );
        }
        for x in 10..=12 {
            assert!(covers(git_rect, x), "git label column {x} not covered");
        }
        assert!(
            !covers(spaces_rect, 10),
            "the spaces tab must not swallow clicks on the git label"
        );
    }

    /// Herdr is mouse-first: the stage/discard click targets must actually be drawn, and the
    /// file name must never spill under them.
    #[test]
    fn file_rows_render_visible_action_buttons_without_overlapping_the_name() {
        use crate::app::state::{AppState, SidebarSpacesView};
        use crate::terminal::TerminalRuntimeRegistry;
        use crate::workspace::{GitFileEntry, GitFileStatusKind, GitWorkingTreeStatus, Workspace};
        use ratatui::{backend::TestBackend, Terminal};

        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("repo")];
        app.active = Some(0);
        app.sidebar_spaces_view = SidebarSpacesView::Git;
        app.git_sidebar.status = Some(GitWorkingTreeStatus {
            staged: vec![GitFileEntry {
                path: "staged_file.rs".into(),
                original_path: None,
                status: GitFileStatusKind::Added,
            }],
            unstaged: vec![GitFileEntry {
                // Long enough to run under the buttons if it were not truncated.
                path: "a/very/long/unstaged/path/name.rs".into(),
                original_path: None,
                status: GitFileStatusKind::Modified,
            }],
        });

        // Wide enough to stay on the desktop layout (a narrow area switches `compute_view` to the
        // mobile path, which has no sidebar at all).
        let area = Rect::new(0, 0, 106, 30);
        crate::ui::compute_view(&mut app, area);
        let mut terminal = Terminal::new(TestBackend::new(106, 30)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::render_with_runtime_registry(
                    &app,
                    &TerminalRuntimeRegistry::new(),
                    frame,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();

        let rows = app.view.git_file_row_areas.clone();
        assert_eq!(rows.len(), 2, "both files should have a row");

        let cell = |x: u16, y: u16| buffer[(x, y)].symbol().to_string();
        for row in &rows {
            let toggle_x = row.stage_toggle_rect.x + row.stage_toggle_rect.width / 2;
            let expected_toggle = if row.staged { "-" } else { "+" };
            assert_eq!(
                cell(toggle_x, row.row_rect.y),
                expected_toggle,
                "stage/unstage button should be drawn for {:?}",
                row.path
            );

            let discard_x = row.discard_rect.x + row.discard_rect.width / 2;
            let discard = cell(discard_x, row.row_rect.y);
            if row.staged {
                assert_eq!(discard, " ", "staged rows must not offer discard");
            } else {
                assert_eq!(discard, "x", "unstaged rows should offer discard");
            }

            // Nothing from the file name may bleed into the action columns: everything from the
            // first action column onward must be a button glyph or blank.
            let row_end = row.row_rect.x + row.row_rect.width;
            for x in row.stage_toggle_rect.x..row_end {
                let symbol = cell(x, row.row_rect.y);
                assert!(
                    matches!(symbol.as_str(), " " | "+" | "-" | "x"),
                    "file name bled into the action columns at x={x}: {symbol:?}"
                );
            }
        }
    }

    /// `git restore` cannot remove an untracked file, so the row must not offer discard at all —
    /// no glyph, and no click target that would only ever produce a git error.
    #[test]
    fn untracked_rows_offer_no_discard_affordance() {
        use crate::app::state::{AppState, SidebarSpacesView};
        use crate::terminal::TerminalRuntimeRegistry;
        use crate::workspace::{GitFileEntry, GitFileStatusKind, GitWorkingTreeStatus, Workspace};
        use ratatui::{backend::TestBackend, Terminal};

        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("repo")];
        app.active = Some(0);
        app.sidebar_spaces_view = SidebarSpacesView::Git;
        app.git_sidebar.status = Some(GitWorkingTreeStatus {
            staged: Vec::new(),
            unstaged: vec![
                GitFileEntry {
                    path: "tracked.rs".into(),
                    original_path: None,
                    status: GitFileStatusKind::Modified,
                },
                GitFileEntry {
                    path: "untracked.rs".into(),
                    original_path: None,
                    status: GitFileStatusKind::Untracked,
                },
            ],
        });

        let area = Rect::new(0, 0, 106, 30);
        crate::ui::compute_view(&mut app, area);
        let mut terminal = Terminal::new(TestBackend::new(106, 30)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::render_with_runtime_registry(
                    &app,
                    &TerminalRuntimeRegistry::new(),
                    frame,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();

        let rows = app.view.git_file_row_areas.clone();
        let tracked = rows
            .iter()
            .find(|row| row.path == "tracked.rs")
            .expect("tracked row");
        let untracked = rows
            .iter()
            .find(|row| row.path == "untracked.rs")
            .expect("untracked row");

        assert!(tracked.can_discard);
        assert!(tracked.discard_rect.width > 0);
        assert_eq!(
            buffer[(
                tracked.discard_rect.x + tracked.discard_rect.width / 2,
                tracked.row_rect.y
            )]
                .symbol(),
            "x"
        );

        assert!(!untracked.can_discard);
        assert_eq!(
            untracked.discard_rect.width, 0,
            "an untracked row must expose no discard click target"
        );
        // Staging is still offered.
        assert_eq!(
            buffer[(
                untracked.stage_toggle_rect.x + untracked.stage_toggle_rect.width / 2,
                untracked.row_rect.y
            )]
                .symbol(),
            "+"
        );
    }

    /// Files past the panel's bottom edge must be reachable: the list scrolls, and the cursor
    /// stays on screen when it moves beyond the fold.
    #[test]
    fn file_list_scrolls_so_rows_past_the_fold_stay_reachable() {
        use crate::app::state::{AppState, SidebarSpacesView};
        use crate::workspace::{GitFileEntry, GitFileStatusKind, GitWorkingTreeStatus, Workspace};

        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("repo")];
        app.active = Some(0);
        app.sidebar_spaces_view = SidebarSpacesView::Git;
        app.git_sidebar.status = Some(GitWorkingTreeStatus {
            staged: Vec::new(),
            unstaged: (0..40)
                .map(|i| GitFileEntry {
                    path: format!("file_{i:02}.rs"),
                    original_path: None,
                    status: GitFileStatusKind::Modified,
                })
                .collect(),
        });

        let area = Rect::new(0, 0, 106, 30);
        crate::ui::compute_view(&mut app, area);
        let visible_at_top: Vec<String> = app
            .view
            .git_file_row_areas
            .iter()
            .map(|row| row.path.clone())
            .collect();
        assert!(
            visible_at_top.len() < 40,
            "the panel cannot show all 40 rows at once"
        );
        assert_eq!(visible_at_top.first().unwrap(), "file_00.rs");
        assert!(
            !visible_at_top.iter().any(|p| p == "file_39.rs"),
            "the last file starts below the fold"
        );

        // Moving the cursor to the last row must bring it into view.
        app.git_sidebar.selected.select(39);
        crate::ui::compute_view(&mut app, area);
        let visible_at_bottom: Vec<String> = app
            .view
            .git_file_row_areas
            .iter()
            .map(|row| row.path.clone())
            .collect();
        assert!(
            visible_at_bottom.iter().any(|p| p == "file_39.rs"),
            "the selected row must be scrolled into view: {visible_at_bottom:?}"
        );
        assert!(app.git_sidebar.scroll > 0, "the list should have scrolled");
    }

    #[test]
    fn tab_hit_areas_are_clamped_to_a_narrow_sidebar() {
        let hits = sidebar_tab_hit_areas(Rect::new(0, 0, 5, 1));
        for (_, rect) in hits {
            assert!(rect.x + rect.width <= 5, "hit area overflows the sidebar");
        }
    }
}
