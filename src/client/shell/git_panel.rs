use super::*;
use ratatui::style::Color;

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

/// Renders the sidebar's Git panel for the focused workspace, and returns the hit-test rect for
/// each rendered file row keyed by its index into `panel_lines`'s `File` entries.
pub(super) fn render_git_panel(
    buffer: &mut Buffer,
    area: Rect,
    workspace: Option<&crate::protocol::ClientShellWorkspace>,
    git_panel: &ClientGitPanelState,
    palette: &Palette,
) -> Vec<(Rect, usize)> {
    if area.is_empty() {
        return Vec::new();
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
        return Vec::new();
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
        return Vec::new();
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
        return Vec::new();
    }

    let Some(working_tree) = workspace.git_working_tree.as_ref() else {
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            " loading…",
            Style::default().fg(palette.overlay0),
        );
        return Vec::new();
    };

    let lines = panel_lines(working_tree);
    if lines.is_empty() {
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            " no changes",
            Style::default().fg(palette.overlay0),
        );
        return Vec::new();
    }

    let max_scroll = lines.len().saturating_sub(1);
    let mut hits = Vec::new();
    for line in lines.iter().skip(git_panel.scroll.min(max_scroll)) {
        if y >= area.bottom() {
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
                    2,
                    &format!(" {glyph}"),
                    Style::default().fg(glyph_color),
                );
                put_text(
                    buffer,
                    rect.x.saturating_add(2),
                    rect.y,
                    rect.width.saturating_sub(2),
                    &entry.path,
                    Style::default().fg(palette.text),
                );
                hits.push((rect, *index));
            }
        }
        y = y.saturating_add(1);
    }
    hits
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

        let hits = render_git_panel(&mut buffer, area, None, &git_panel, &palette);

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

        let hits = render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

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

        let hits = render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

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

        let hits = render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

        assert!(hits.is_empty());
        assert!(buffer_text(&buffer).contains("no changes"));
    }

    #[test]
    fn renders_branch_ahead_behind_and_file_rows() {
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
            unstaged: vec![ClientShellGitFileEntry {
                path: "unstaged.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Modified,
            }],
        };
        let ws = workspace(true, Some(working_tree));

        let hits = render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

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
        };
        let hits = render_git_panel(&mut buffer_scrolled, area, Some(&ws), &scrolled, &palette);
        let text = buffer_text(&buffer_scrolled);
        assert!(!text.contains("STAGED CHANGES"));
        assert!(text.contains("staged.rs"));
        assert_eq!(hits.len(), 1);
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
            scroll: 0,
        };

        let hits = render_git_panel(&mut buffer, area, Some(&ws), &git_panel, &palette);

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
