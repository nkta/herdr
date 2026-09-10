use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::widgets::{panel_contrast_fg, render_panel_shell};
use crate::app::AppState;

fn prefix_rhs_label(bindings: &crate::config::ActionKeybinds) -> String {
    bindings
        .prefix_rhs_label()
        .unwrap_or_else(|| "unset".to_string())
}

fn keybind_label(bindings: &crate::config::ActionKeybinds) -> String {
    bindings.label().unwrap_or_else(|| "unset".to_string())
}

fn render_bottom_bar(frame: &mut Frame, area: Rect, line: Line<'_>, bg: ratatui::style::Color) {
    frame.render_widget(Clear, area);
    let buf = frame.buffer_mut();
    for x in area.x..area.x + area.width {
        buf[(x, area.y)].set_style(Style::default().bg(bg));
    }
    frame.render_widget(Paragraph::new(line), area);
}

/// Columns are laid out from the widest entry in each group, so a group whose
/// labels are short does not reserve the space the longest group needs.
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

fn prefix_hint_columns(app: &AppState, max_rows: usize) -> Vec<PrefixHintColumn> {
    let max_rows = max_rows.max(1);
    let mut columns = Vec::new();
    for (title, entries) in super::keybind_help::prefix_hint_groups(app) {
        let entries: Vec<(String, String)> = entries
            .into_iter()
            .map(|(keys, label)| (keys, label.into_owned()))
            .collect();
        for (index, chunk) in entries.chunks(max_rows).enumerate() {
            let width = chunk
                .iter()
                .map(|(keys, label)| keys.chars().count() + label.chars().count() + 2)
                .chain(std::iter::once(title.chars().count()))
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

/// Fit as many whole columns as the width allows, widest-first order preserved.
fn fit_prefix_hint_columns(
    columns: Vec<PrefixHintColumn>,
    available: u16,
) -> Vec<PrefixHintColumn> {
    let mut used = 0u16;
    let mut fitted = Vec::new();
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
/// little of the panes as the entries allow.
pub(super) fn render_prefix_hint_panel(app: &AppState, frame: &mut Frame, area: Rect) -> bool {
    let key = Style::default()
        .fg(app.palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(app.palette.overlay0);
    let title_style = Style::default()
        .fg(app.palette.text)
        .add_modifier(Modifier::BOLD);

    // Two border rows, one title row, one blank row and one footer row frame the entries.
    let chrome_height = 5u16;
    if area.height <= chrome_height || area.width < PREFIX_HINT_MIN_COLUMN_WIDTH + 2 {
        return false;
    }

    let rows_available = (area.height - chrome_height) as usize;
    let max_rows = rows_available.min(PREFIX_HINT_MAX_ROWS);
    let columns = fit_prefix_hint_columns(
        prefix_hint_columns(app, max_rows),
        area.width.saturating_sub(2),
    );
    if columns.is_empty() {
        return false;
    }

    let rows_needed = columns
        .iter()
        .map(|column| column.entries.len())
        .max()
        .unwrap_or(0)
        .min(max_rows);
    if rows_needed == 0 {
        return false;
    }

    let panel_height = rows_needed as u16 + chrome_height;
    let panel = Rect::new(
        area.x,
        area.y + area.height - panel_height,
        area.width,
        panel_height,
    );
    let Some(inner) = render_panel_shell(frame, panel, app.palette.accent, app.palette.panel_bg)
    else {
        return false;
    };

    // Name the panel on its own top border, the way the mode bar names PREFIX.
    if panel.width > 10 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(" PREFIX ", key))),
            Rect::new(panel.x + 2, panel.y, panel.width - 4, 1),
        );
    }

    let mut lines: Vec<Line> = Vec::with_capacity(rows_needed + 2);
    let last_title = columns.len() - 1;
    lines.push(Line::from(
        columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let width = prefix_hint_cell_width(column.width, index == last_title);
                Span::styled(pad_prefix_hint_cell(column.title, width), title_style)
            })
            .collect::<Vec<_>>(),
    ));

    let last_column = columns.len() - 1;
    for row in 0..rows_needed {
        let mut spans = Vec::with_capacity(columns.len() * 3);
        for (index, column) in columns.iter().enumerate() {
            let cell_width = prefix_hint_cell_width(column.width, index == last_column);
            match column.entries.get(row) {
                Some((keys, label)) => {
                    spans.push(Span::styled(keys.clone(), key));
                    spans.push(Span::styled(format!("  {label}"), dim));
                    let used = keys.chars().count() + 2 + label.chars().count();
                    if cell_width > used {
                        spans.push(Span::raw(" ".repeat(cell_width - used)));
                    }
                }
                None if index < last_column => spans.push(Span::raw(" ".repeat(cell_width))),
                None => {}
            }
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::default());
    let prefix = crate::config::format_key_combo((app.prefix_code, app.prefix_mods));
    lines.push(Line::from(vec![
        Span::styled("esc", key),
        Span::styled(" cancel   ", dim),
        Span::styled(prefix, key),
        Span::styled(" send prefix   ", dim),
        Span::styled(prefix_rhs_label(&app.keybinds.help), key),
        Span::styled(" all keybinds", dim),
    ]));

    frame.render_widget(Paragraph::new(lines), inner);
    true
}

/// The gap belongs to every column but the last, which must not pad past the
/// panel edge and wrap the line.
fn prefix_hint_cell_width(column_width: u16, is_last: bool) -> usize {
    let gap = if is_last { 0 } else { PREFIX_HINT_COLUMN_GAP };
    column_width as usize + gap as usize
}

fn pad_prefix_hint_cell(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.chars().take(width).collect();
    }
    let mut padded = String::with_capacity(width);
    padded.push_str(text);
    padded.extend(std::iter::repeat_n(' ', width - len));
    padded
}

pub(super) fn render_prefix_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    if app.prefix_hint_visible && render_prefix_hint_panel(app, frame, area) {
        return;
    }

    let key = Style::default()
        .fg(app.palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(app.palette.overlay0);
    let mode_style = Style::default()
        .fg(panel_contrast_fg(&app.palette))
        .bg(app.palette.accent)
        .add_modifier(Modifier::BOLD);

    let workspace_picker = prefix_rhs_label(&app.keybinds.workspace_picker);
    let help = prefix_rhs_label(&app.keybinds.help);
    let prefix = crate::config::format_key_combo((app.prefix_code, app.prefix_mods));

    let line = Line::from(vec![
        Span::styled(" PREFIX ", mode_style),
        Span::raw(" "),
        Span::styled("esc", key),
        Span::styled(" cancel  ", dim),
        Span::styled(prefix, key),
        Span::styled(" send prefix  ", dim),
        Span::styled(workspace_picker, key),
        Span::styled(" workspace nav  ", dim),
        Span::styled(help, key),
        Span::styled(" keybinds", dim),
    ]);

    let overlay_y = area.y + area.height.saturating_sub(1);
    let overlay_area = Rect::new(area.x, overlay_y, area.width, 1);
    render_bottom_bar(frame, overlay_area, line, app.palette.panel_bg);
}

pub(super) fn render_copy_mode_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let key = Style::default()
        .fg(app.palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(app.palette.overlay0);
    let mode_style = Style::default()
        .fg(panel_contrast_fg(&app.palette))
        .bg(app.palette.accent)
        .add_modifier(Modifier::BOLD);

    let Some(copy_mode) = app.copy_mode.as_ref() else {
        return;
    };
    let line = if let Some(prompt) = copy_mode.search.prompt.as_ref() {
        let marker = match prompt.direction {
            crate::app::state::CopyModeSearchDirection::Forward => "/",
            crate::app::state::CopyModeSearchDirection::Backward => "?",
        };
        Line::from(vec![
            Span::styled(" COPY ", mode_style),
            Span::raw(" "),
            Span::styled(marker, key),
            Span::styled(prompt.query.clone(), Style::default().fg(app.palette.text)),
            Span::styled("█", key),
            Span::styled("  enter search  esc cancel", dim),
        ])
    } else {
        let select = if copy_mode.selection.is_some() {
            "selecting"
        } else {
            "select"
        };
        let match_status = copy_mode
            .search
            .current
            .map(|current| format!(" {}/{}", current + 1, copy_mode.search.matches.len()))
            .or_else(|| (!copy_mode.search.query.is_empty()).then(|| " 0/0".to_string()))
            .unwrap_or_default();
        let (exit_keys, exit_label) =
            if copy_mode.search.query.is_empty() && copy_mode.selection.is_none() {
                ("q/esc", " exit")
            } else {
                ("esc", " clear  q exit")
            };
        Line::from(vec![
            Span::styled(" COPY ", mode_style),
            Span::raw(" "),
            Span::styled("h/j/k/l w/b/e { }", key),
            Span::styled(" move  ", dim),
            Span::styled("/ ?", key),
            Span::styled(" search  ", dim),
            Span::styled("n/N", key),
            Span::styled(format!(" repeat{match_status}  "), dim),
            Span::styled("v/space", key),
            Span::styled(format!(" {select}  "), dim),
            Span::styled("y/enter", key),
            Span::styled(" copy  ", dim),
            Span::styled(exit_keys, key),
            Span::styled(exit_label, dim),
        ])
    };

    let overlay_y = area.y + area.height.saturating_sub(1);
    let overlay_area = Rect::new(area.x, overlay_y, area.width, 1);
    render_bottom_bar(frame, overlay_area, line, app.palette.panel_bg);
}

pub(super) fn render_navigate_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let key = Style::default()
        .fg(app.palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(app.palette.overlay0);

    let mode_style = Style::default()
        .fg(panel_contrast_fg(&app.palette))
        .bg(app.palette.accent)
        .add_modifier(Modifier::BOLD);

    let kb = &app.keybinds;
    let new_tab = prefix_rhs_label(&kb.new_tab);
    let split_vertical = prefix_rhs_label(&kb.split_vertical);
    let split_horizontal = prefix_rhs_label(&kb.split_horizontal);
    let close_pane = prefix_rhs_label(&kb.close_pane);
    let zoom = prefix_rhs_label(&kb.zoom);
    let resize = prefix_rhs_label(&kb.resize_mode);
    let help = prefix_rhs_label(&kb.help);
    let settings = prefix_rhs_label(&kb.settings);
    let goto = prefix_rhs_label(&kb.goto);
    let detach = prefix_rhs_label(&kb.detach);
    let workspace_nav = format!(
        "{} / {}",
        keybind_label(&kb.navigate.workspace_up),
        keybind_label(&kb.navigate.workspace_down)
    );
    let line = Line::from(vec![
        Span::styled(" NAVIGATE ", mode_style),
        Span::raw(" "),
        Span::styled("esc", key),
        Span::styled(" back  ", dim),
        Span::styled(workspace_nav, key),
        Span::styled(" ws  ", dim),
        Span::styled("⇥", key),
        Span::styled(" pane  ", dim),
        Span::styled(goto, key),
        Span::styled(" navigator  ", dim),
        Span::styled(new_tab, key),
        Span::styled(" new tab  ", dim),
        Span::styled(split_vertical, key),
        Span::styled(" split│  ", dim),
        Span::styled(split_horizontal, key),
        Span::styled(" split─  ", dim),
        Span::styled(close_pane, key),
        Span::styled(" close  ", dim),
        Span::styled(zoom, key),
        Span::styled(" zoom  ", dim),
        Span::styled(resize, key),
        Span::styled(" resize  ", dim),
        Span::styled(help, key),
        Span::styled(" keybinds  ", dim),
        Span::styled(settings, key),
        Span::styled(" settings  ", dim),
        Span::styled(detach, key),
        Span::styled(" detach", dim),
    ]);

    let overlay_y = area.y + area.height.saturating_sub(1);
    let overlay_area = Rect::new(area.x, overlay_y, area.width, 1);
    render_bottom_bar(frame, overlay_area, line, app.palette.panel_bg);

    if app.update_available.is_some() {
        let status = Line::from(vec![Span::styled(
            " update ready",
            Style::default()
                .fg(app.palette.accent)
                .add_modifier(Modifier::BOLD),
        )]);
        let width = 13u16.min(overlay_area.width);
        let status_area = Rect::new(
            overlay_area.x + overlay_area.width.saturating_sub(width),
            overlay_area.y,
            width,
            overlay_area.height,
        );
        frame.render_widget(Clear, status_area);
        frame.render_widget(
            Paragraph::new(status).alignment(Alignment::Right),
            status_area,
        );
    }
}

pub(super) fn render_global_launcher_menu(app: &AppState, frame: &mut Frame) {
    let rect = app.global_menu_rect();
    let Some(inner) = render_panel_shell(frame, rect, app.palette.accent, app.palette.panel_bg)
    else {
        return;
    };

    let items = app.global_menu_labels();
    for (idx, item) in items.iter().enumerate() {
        let y = inner.y + idx as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let selected = idx == app.global_menu.highlighted;
        let rect = Rect::new(inner.x, y, inner.width, 1);

        let selected_style = Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD);
        let item_style = if selected {
            selected_style
        } else {
            Style::default().fg(app.palette.text)
        };
        let badge_style = if selected {
            selected_style
        } else {
            Style::default()
                .fg(app.palette.accent)
                .add_modifier(Modifier::BOLD)
        };

        let line = if app.global_menu_item_has_badge(item) {
            Line::from(vec![
                Span::styled(" ●", badge_style),
                Span::styled(format!(" {item} "), item_style),
            ])
        } else {
            Line::from(Span::styled(format!(" {item} "), item_style))
        };
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), rect);
    }
}

pub(super) fn render_resize_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let key = Style::default()
        .fg(app.palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(app.palette.overlay0);

    let mode_style = Style::default()
        .fg(panel_contrast_fg(&app.palette))
        .bg(app.palette.mauve)
        .add_modifier(Modifier::BOLD);

    let line = Line::from(vec![
        Span::styled(" RESIZE ", mode_style),
        Span::raw("  "),
        Span::styled("h/l", key),
        Span::styled(" width  ", dim),
        Span::styled("j/k", key),
        Span::styled(" height  ", dim),
        Span::styled("esc", key),
        Span::styled(" done", dim),
    ]);

    let overlay_y = area.y + area.height.saturating_sub(1);
    let overlay_area = Rect::new(area.x, overlay_y, area.width, 1);
    render_bottom_bar(frame, overlay_area, line, app.palette.panel_bg);
}

pub(super) fn render_context_menu(app: &AppState, frame: &mut Frame) {
    let Some(menu) = &app.context_menu else {
        return;
    };

    let p = &app.palette;
    let Some(menu_rect) = app.context_menu_rect() else {
        return;
    };
    let Some(inner) = render_panel_shell(frame, menu_rect, p.accent, p.panel_bg) else {
        return;
    };

    let items: Vec<ListItem> = menu
        .items()
        .iter()
        .map(|item| ListItem::new(Line::from(*item)))
        .collect();
    let list = List::new(items)
        .style(Style::default().fg(p.text))
        .highlight_style(
            Style::default()
                .bg(p.accent)
                .fg(panel_contrast_fg(p))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ");
    let mut state = ListState::default().with_selected(Some(menu.list.highlighted));
    frame.render_stateful_widget(list, inner, &mut state);
}
