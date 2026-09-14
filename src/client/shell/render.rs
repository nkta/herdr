use super::*;

#[path = "../shell/git_panel.rs"]
mod git_panel;
#[path = "../shell/overlays.rs"]
mod overlays;
#[path = "../shell/sidebar.rs"]
pub(in crate::client::shell) mod sidebar;
#[path = "../shell/tabs.rs"]
mod tabs;

pub(super) use super::agent_sidebar::{ordered_agent_pane_ids, render_agent_panel};
pub(super) use super::aggregate_navigation::navigator_rows as client_navigator_rows;
pub(super) use git_panel::scroll_to_reveal_file as git_panel_scroll_to_reveal_file;
pub(super) use overlays::{render_client_overlay, render_context_menu, render_global_menu};
pub(super) use sidebar::{render_collapsed_sidebar, render_sidebar, workspace_entries};
pub(super) use tabs::{render_tab_bar, tab_bar_status_width};

pub(in crate::client::shell) fn render_sidebar_background(
    buffer: &mut Buffer,
    area: Rect,
    palette: &Palette,
) {
    buffer.set_style(area, Style::default().bg(palette.sidebar_bg));
    let separator_x = area.right().saturating_sub(1);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((separator_x, y)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(palette.surface_dim));
        }
    }
}

pub(super) fn render_mode_bar(
    buffer: &mut Buffer,
    pane_area: Rect,
    mode: ClientShellMode,
    copy_mode: Option<&ClientCopyModeState>,
    endpoint_error: Option<&str>,
    update_available: bool,
    commit_agent_generate_supported: bool,
    keybinds: &LiveKeybindConfig,
    palette: &Palette,
) -> Option<Rect> {
    if (mode == ClientShellMode::Terminal && endpoint_error.is_none()) || pane_area.is_empty() {
        return None;
    }

    let bar = Rect::new(
        pane_area.x,
        pane_area.y + pane_area.height.saturating_sub(1),
        pane_area.width,
        1,
    );
    let base = Style::default().fg(palette.overlay0).bg(palette.panel_bg);
    for x in bar.x..bar.x + bar.width {
        buffer[(x, bar.y)].set_symbol(" ").set_style(base);
    }

    let key = Style::default()
        .fg(palette.accent)
        .bg(palette.panel_bg)
        .add_modifier(Modifier::BOLD);
    let mode_style = Style::default()
        .fg(match palette.panel_bg {
            ratatui::style::Color::Reset => palette.surface_dim,
            color => color,
        })
        .bg(if mode == ClientShellMode::Resize {
            palette.mauve
        } else {
            palette.accent
        })
        .add_modifier(Modifier::BOLD);
    let prefix = crate::config::format_key_combo(keybinds.prefix);
    let prefix_rhs = |bindings: &crate::config::ActionKeybinds| {
        bindings
            .prefix_rhs_label()
            .unwrap_or_else(|| "unset".to_owned())
    };

    let mut segments = Vec::<(String, Style)>::new();
    if let Some(error) = endpoint_error {
        segments.extend([
            (" ERROR ".to_owned(), mode_style),
            (format!(" {error}"), base),
        ]);
    } else {
        match mode {
            ClientShellMode::Prefix => {
                segments.extend([
                    (" PREFIX ".to_owned(), mode_style),
                    (" ".to_owned(), base),
                    ("esc".to_owned(), key),
                    (" cancel  ".to_owned(), base),
                    (prefix, key),
                    (" send prefix  ".to_owned(), base),
                    (prefix_rhs(&keybinds.keybinds.workspace_picker), key),
                    (" workspace nav  ".to_owned(), base),
                    (prefix_rhs(&keybinds.keybinds.help), key),
                    (" keybinds".to_owned(), base),
                ]);
            }
            ClientShellMode::Navigate => {
                segments.extend([
                    (" NAVIGATE ".to_owned(), mode_style),
                    (" esc back  ".to_owned(), base),
                    ("↑/↓".to_owned(), key),
                    (" workspace  ".to_owned(), base),
                    ("tab".to_owned(), key),
                    (" pane  ".to_owned(), base),
                    (prefix_rhs(&keybinds.keybinds.help), key),
                    (" keybinds".to_owned(), base),
                ]);
            }
            ClientShellMode::Resize => {
                segments.extend([
                    (" RESIZE ".to_owned(), mode_style),
                    ("  ".to_owned(), base),
                    ("h/l".to_owned(), key),
                    (" width  ".to_owned(), base),
                    ("j/k".to_owned(), key),
                    (" height  ".to_owned(), base),
                    ("esc".to_owned(), key),
                    (" done".to_owned(), base),
                ]);
            }
            ClientShellMode::Copy => {
                let copy_mode = copy_mode?;
                if let Some(prompt) = copy_mode.search_prompt.as_ref() {
                    let marker = match prompt.direction {
                        crate::api::schema::PaneCopySearchDirection::Forward => "/",
                        crate::api::schema::PaneCopySearchDirection::Backward => "?",
                    };
                    segments.extend([
                        (" COPY ".to_owned(), mode_style),
                        (" ".to_owned(), base),
                        (marker.to_owned(), key),
                        (
                            prompt.query.clone(),
                            Style::default().fg(palette.text).bg(palette.panel_bg),
                        ),
                        ("█".to_owned(), key),
                        ("  enter search  esc cancel".to_owned(), base),
                    ]);
                } else {
                    let select = if copy_mode.selection.is_some() {
                        "selecting"
                    } else {
                        "select"
                    };
                    let match_status = copy_mode
                        .search_current_global
                        .map(|current| format!(" {}/{}", current + 1, copy_mode.search_total))
                        .or_else(|| (!copy_mode.search_query.is_empty()).then(|| " 0/0".to_owned()))
                        .unwrap_or_default();
                    let (exit_keys, exit_label) =
                        if copy_mode.search_query.is_empty() && copy_mode.selection.is_none() {
                            ("q/esc", " exit")
                        } else {
                            ("esc", " clear  q exit")
                        };
                    segments.extend([
                        (" COPY ".to_owned(), mode_style),
                        (" ".to_owned(), base),
                        ("h/j/k/l w/b/e { }".to_owned(), key),
                        (" move  ".to_owned(), base),
                        ("/ ?".to_owned(), key),
                        (" search  ".to_owned(), base),
                        ("n/N".to_owned(), key),
                        (format!(" repeat{match_status}  "), base),
                        ("v/space".to_owned(), key),
                        (format!(" {select}  "), base),
                        ("y/enter".to_owned(), key),
                        (" copy  ".to_owned(), base),
                        (exit_keys.to_owned(), key),
                        (exit_label.to_owned(), base),
                    ]);
                }
            }
            ClientShellMode::SidebarGit => {
                segments.extend([
                    (" GIT ".to_owned(), mode_style),
                    (" ".to_owned(), base),
                    ("tab".to_owned(), key),
                    (" file list/commit  ".to_owned(), base),
                    ("j/k".to_owned(), key),
                    (" select  ".to_owned(), base),
                    ("s/u/d".to_owned(), key),
                    (" stage/unstage/discard  ".to_owned(), base),
                    ("enter".to_owned(), key),
                    (" diff  ".to_owned(), base),
                    ("m".to_owned(), key),
                    (" menu  ".to_owned(), base),
                    ("ctrl+enter".to_owned(), key),
                    (" commit  ".to_owned(), base),
                ]);
                if commit_agent_generate_supported {
                    segments.extend([("ctrl+g".to_owned(), key), (" generate  ".to_owned(), base)]);
                }
                segments.extend([("esc".to_owned(), key), (" back".to_owned(), base)]);
            }
            ClientShellMode::Terminal => unreachable!(),
        }
    }

    let mut x = bar.x;
    let end = bar.x + bar.width;
    for (text, style) in segments {
        if x >= end {
            break;
        }
        let remaining = end - x;
        buffer.set_stringn(x, bar.y, &text, usize::from(remaining), style);
        x = x.saturating_add(
            u16::try_from(UnicodeWidthStr::width(text.as_str()))
                .unwrap_or(u16::MAX)
                .min(remaining),
        );
    }
    if update_available && mode == ClientShellMode::Navigate {
        let width = 13.min(bar.width);
        let area = Rect::new(bar.right().saturating_sub(width), bar.y, width, 1);
        buffer.set_style(area, Style::default().bg(palette.panel_bg));
        put_right_text(
            buffer,
            area,
            area.y,
            " update ready",
            Style::default()
                .fg(palette.accent)
                .bg(palette.panel_bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    Some(bar)
}

pub(super) struct ShellRenderState<'a> {
    pub(super) endpoints: &'a [ClientShellEndpoint],
    pub(super) active_endpoint_id: &'a ClientEndpointId,
    pub(super) collapsed_endpoints: &'a HashSet<ClientEndpointId>,
    pub(super) collapsed_groups: &'a HashSet<String>,
    pub(super) remote_collapsed_groups: &'a HashMap<ClientEndpointId, HashSet<String>>,
    pub(super) workspace_scroll: &'a mut usize,
    pub(super) agent_scroll: &'a mut usize,
    pub(super) tab_scroll: &'a mut usize,
    pub(super) reveal_focused_workspace: &'a mut bool,
    pub(super) reveal_focused_tab: &'a mut bool,
    pub(super) sidebar_collapsed: bool,
    pub(super) sidebar_section_split: f32,
    pub(super) tab_drag_insert_index: Option<usize>,
    pub(super) selected_workspace_id: Option<&'a WorkspaceNavigationTarget>,
    pub(super) reveal_navigation_workspace: &'a mut bool,
    pub(super) dragged_workspace_id: Option<&'a str>,
    pub(super) workspace_drop_indicator_row: Option<u16>,
    pub(super) sidebar_view: SidebarSpacesView,
    pub(super) git_panel: &'a ClientGitPanelState,
}

pub(super) fn render_shell(
    buffer: &mut Buffer,
    layout: ClientShellLayout,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    mut state: ShellRenderState<'_>,
) -> ShellHitMap {
    let mut hits = ShellHitMap::default();
    if layout.mobile_header.height > 0 {
        super::mobile::render_mobile_header(
            buffer,
            layout.mobile_header,
            snapshot,
            config,
            &mut hits,
        );
    }
    if layout.sidebar.width > 0 {
        if state.endpoints.len() > 1 {
            if state.sidebar_collapsed {
                super::endpoint_sidebar::render_collapsed(
                    buffer,
                    layout.sidebar,
                    config,
                    &mut state,
                    &mut hits,
                );
            } else {
                super::endpoint_sidebar::render_expanded(
                    buffer,
                    layout.sidebar,
                    Some(snapshot),
                    config,
                    &mut state,
                    &mut hits,
                );
            }
        } else if state.sidebar_collapsed {
            render_collapsed_sidebar(
                buffer,
                layout.sidebar,
                snapshot,
                config,
                state
                    .selected_workspace_id
                    .map(|target| target.workspace_id.as_str()),
                &mut hits,
            );
        } else {
            render_sidebar(
                buffer,
                layout.sidebar,
                snapshot,
                config,
                &mut state,
                &mut hits,
            );
        }
    }
    if layout.tab_bar.height > 0 {
        render_tab_bar(
            buffer,
            layout.tab_bar,
            snapshot,
            config,
            state.tab_scroll,
            state.reveal_focused_tab,
            state.tab_drag_insert_index,
            &mut hits,
        );
    }
    if !config.mouse_capture {
        hits.sidebar_divider = Rect::default();
        hits.sidebar_section_divider = Rect::default();
        hits.workspace_scrollbar = Rect::default();
        hits.agent_scrollbar = Rect::default();
        hits.agent_sort_toggle = Rect::default();
        hits.new_workspace = Rect::default();
        hits.machines.clear();
        hits.workspaces.clear();
        hits.agents.clear();
        hits.endpoint_agents.clear();
        hits.tab_scroll_left = Rect::default();
        hits.tab_scroll_right = Rect::default();
        hits.new_tab = Rect::default();
        hits.pane_splits.clear();
    }
    hits
}

pub(super) fn put_right_text(buffer: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
    let width = display_width(text).min(area.width);
    put_text(
        buffer,
        area.right().saturating_sub(width),
        y,
        width,
        text,
        style,
    );
}

pub(super) fn put_segment(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    right: u16,
    text: &str,
    style: Style,
) -> u16 {
    let width = display_width(text).min(right.saturating_sub(x));
    put_text(buffer, x, y, width, text, style);
    x.saturating_add(width)
}

pub(super) fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    if width == 0 || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    buffer.set_stringn(x, y, text, width as usize, style);
}

pub(super) fn display_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}

// --- prefix keybinding panel -------------------------------------------------

/// Columns are sized from the widest entry in each group, so a group with short
/// labels does not reserve the space the longest group needs.
const PREFIX_HINT_COLUMN_GAP: u16 = 2;
const PREFIX_HINT_MIN_COLUMN_WIDTH: u16 = 14;
/// A group longer than this continues in the next column rather than making the
/// panel tall enough to bury the panes it is supposed to help you work in.
const PREFIX_HINT_MAX_ROWS: usize = 12;

struct PrefixHintColumn {
    title: &'static str,
    entries: Vec<(String, String)>,
    width: u16,
}

fn prefix_hint_columns(keybinds: &LiveKeybindConfig, max_rows: usize) -> Vec<PrefixHintColumn> {
    let max_rows = max_rows.max(1);
    let mut columns = Vec::new();
    for (title, entries) in crate::input::prefix_hint_groups(&keybinds.keybinds, keybinds.prefix) {
        let entries: Vec<(String, String)> = entries
            .into_iter()
            .map(|(keys, label)| (keys, label.into_owned()))
            .collect();
        for (index, chunk) in entries.chunks(max_rows).enumerate() {
            let width = chunk
                .iter()
                .map(|(keys, label)| {
                    usize::from(display_width(keys)) + usize::from(display_width(label)) + 2
                })
                .chain(std::iter::once(usize::from(display_width(title))))
                .max()
                .unwrap_or(0)
                .min(u16::MAX as usize) as u16;
            columns.push(PrefixHintColumn {
                // Only the first column of a wrapped group is titled; a repeated
                // heading would read as a second, different group.
                title: if index == 0 { title } else { "" },
                entries: chunk.to_vec(),
                width: width.max(PREFIX_HINT_MIN_COLUMN_WIDTH),
            });
        }
    }
    columns
}

/// Fit as many whole columns as the width allows, in order.
fn fit_prefix_hint_columns(
    columns: Vec<PrefixHintColumn>,
    available: u16,
) -> Vec<PrefixHintColumn> {
    let mut used = 0u16;
    let mut fitted: Vec<PrefixHintColumn> = Vec::new();
    for column in columns {
        let needed = if fitted.is_empty() {
            column.width
        } else {
            column.width.saturating_add(PREFIX_HINT_COLUMN_GAP)
        };
        if used.saturating_add(needed) > available {
            break;
        }
        used = used.saturating_add(needed);
        fitted.push(column);
    }
    fitted
}

/// The keybinding panel prefix mode shows once it has been held past the delay.
///
/// Anchored to the bottom of `area` and sized to its content, so it covers as
/// little of the panes as the entries allow. Returns the rect it painted, which
/// the caller has to restore after the pane surface is blitted over it.
pub(super) fn render_prefix_hint_panel(
    buffer: &mut Buffer,
    area: Rect,
    keybinds: &LiveKeybindConfig,
    palette: &Palette,
) -> Option<Rect> {
    // Two border rows, one title row, one blank row and one footer row frame the entries.
    const CHROME_HEIGHT: u16 = 5;
    if area.height <= CHROME_HEIGHT || area.width < PREFIX_HINT_MIN_COLUMN_WIDTH + 2 {
        return None;
    }

    let rows_available = usize::from(area.height - CHROME_HEIGHT);
    let max_rows = rows_available.min(PREFIX_HINT_MAX_ROWS);
    let columns = fit_prefix_hint_columns(
        prefix_hint_columns(keybinds, max_rows),
        area.width.saturating_sub(2),
    );
    if columns.is_empty() {
        return None;
    }
    let rows_needed = columns
        .iter()
        .map(|column| column.entries.len())
        .max()
        .unwrap_or(0)
        .min(max_rows);
    if rows_needed == 0 {
        return None;
    }

    let panel_rect = Rect::new(
        area.x,
        area.y + area.height - (rows_needed as u16 + CHROME_HEIGHT),
        area.width,
        rows_needed as u16 + CHROME_HEIGHT,
    );
    let inner = prefix_hint_panel_shell(buffer, panel_rect, palette)?;

    let title_style = Style::default()
        .fg(palette.text)
        .bg(palette.panel_bg)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default()
        .fg(palette.accent)
        .bg(palette.panel_bg)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(palette.overlay0).bg(palette.panel_bg);

    // Name the panel on its own top border, the way the mode bar names PREFIX.
    if panel_rect.width > 10 {
        put_text(
            buffer,
            panel_rect.x + 2,
            panel_rect.y,
            panel_rect.width - 4,
            " PREFIX ",
            key_style,
        );
    }

    let last = columns.len() - 1;
    let mut x = inner.x;
    for (index, column) in columns.iter().enumerate() {
        let cell_width = column.width
            + if index == last {
                0
            } else {
                PREFIX_HINT_COLUMN_GAP
            };
        put_text(buffer, x, inner.y, column.width, column.title, title_style);
        for (row, (keys, label)) in column.entries.iter().enumerate() {
            let y = inner.y + 1 + row as u16;
            let key_width = display_width(keys);
            put_text(buffer, x, y, key_width, keys, key_style);
            put_text(
                buffer,
                x + key_width,
                y,
                column.width.saturating_sub(key_width),
                &format!("  {label}"),
                dim,
            );
        }
        x += cell_width;
    }

    let footer_y = inner.bottom() - 1;
    let prefix = crate::config::format_key_combo(keybinds.prefix);
    let mut x = inner.x;
    for (text, style) in [
        ("esc", key_style),
        (" cancel   ", dim),
        (prefix.as_str(), key_style),
        (" send prefix", dim),
    ] {
        let width = display_width(text).min(inner.right().saturating_sub(x));
        put_text(buffer, x, footer_y, width, text, style);
        x += width;
    }

    Some(panel_rect)
}

fn prefix_hint_panel_shell(buffer: &mut Buffer, area: Rect, palette: &Palette) -> Option<Rect> {
    if area.width < 2 || area.height < 2 {
        return None;
    }
    let background = Style::default()
        .bg(palette.panel_bg)
        .remove_modifier(Modifier::DIM);
    let border = Style::default()
        .fg(palette.accent)
        .bg(palette.panel_bg)
        .remove_modifier(Modifier::DIM);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buffer[(x, y)].set_symbol(" ").set_style(background);
        }
    }
    for x in area.x..area.right() {
        let top = if x == area.x {
            "┌"
        } else if x + 1 == area.right() {
            "┐"
        } else {
            "─"
        };
        let bottom = if x == area.x {
            "└"
        } else if x + 1 == area.right() {
            "┘"
        } else {
            "─"
        };
        buffer[(x, area.y)].set_symbol(top).set_style(border);
        buffer[(x, area.bottom() - 1)]
            .set_symbol(bottom)
            .set_style(border);
    }
    for y in area.y + 1..area.bottom() - 1 {
        buffer[(area.x, y)].set_symbol("│").set_style(border);
        buffer[(area.right() - 1, y)]
            .set_symbol("│")
            .set_style(border);
    }
    Some(Rect::new(
        area.x + 1,
        area.y + 1,
        area.width - 2,
        area.height - 2,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_keybinds() -> LiveKeybindConfig {
        Config::default()
            .live_keybinds_with_diagnostics()
            .unwrap()
            .0
    }

    fn test_palette() -> Palette {
        crate::app::client_palette_from_config(&Config::default())
    }

    fn bar_text(buffer: &Buffer, bar: Rect) -> String {
        (bar.x..bar.right())
            .map(|x| buffer[(x, bar.y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn sidebar_git_mode_bar_shows_commit_and_generate_shortcuts() {
        let palette = test_palette();
        let keybinds = test_keybinds();
        let area = Rect::new(0, 0, 200, 10);
        let mut buffer = Buffer::empty(area);

        let bar = render_mode_bar(
            &mut buffer,
            area,
            ClientShellMode::SidebarGit,
            None,
            None,
            false,
            true,
            &keybinds,
            &palette,
        )
        .unwrap();

        let text = bar_text(&buffer, bar);
        assert!(text.contains("ctrl+enter"));
        assert!(text.contains("commit"));
        assert!(text.contains("ctrl+g"));
        assert!(text.contains("generate"));
    }

    #[test]
    fn sidebar_git_mode_bar_hides_generate_when_unsupported() {
        let palette = test_palette();
        let keybinds = test_keybinds();
        let area = Rect::new(0, 0, 200, 10);
        let mut buffer = Buffer::empty(area);

        let bar = render_mode_bar(
            &mut buffer,
            area,
            ClientShellMode::SidebarGit,
            None,
            None,
            false,
            false,
            &keybinds,
            &palette,
        )
        .unwrap();

        let text = bar_text(&buffer, bar);
        assert!(text.contains("ctrl+enter"));
        assert!(!text.contains("ctrl+g"));
    }
}
