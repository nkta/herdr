use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::state::GitPickerState;
use crate::app::AppState;

use super::text::truncate_end;
use super::widgets::{centered_popup_rect, modal_stack_areas, render_panel_shell};

const PICKER_WIDTH: u16 = 62;
const PICKER_HEIGHT: u16 = 18;

/// Modal list picker for choosing a stash or a branch, built from the same popup/panel chrome as
/// the worktree dialogs so it reads as part of the existing UI language.
pub(super) fn render_git_picker_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let Some(picker) = app.git_picker.as_ref() else {
        return;
    };
    let p = &app.palette;
    let Some(popup) = centered_popup_rect(area, PICKER_WIDTH, PICKER_HEIGHT) else {
        return;
    };

    super::dim_background(frame, area);
    let Some(inner) = render_panel_shell(frame, popup, p.accent, p.panel_bg) else {
        return;
    };
    if inner.height < 4 || inner.width < 10 {
        return;
    }

    let stack = modal_stack_areas(inner, 2, 2, 0, 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!(" {}", picker.kind.title()),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        )])),
        Rect::new(stack.header.x, stack.header.y, stack.header.width, 1),
    );

    render_body(picker, frame, stack.content, app);

    if let Some(footer) = stack.footer {
        frame.render_widget(
            Paragraph::new(Span::styled(
                " enter apply   esc cancel   ↑/↓ move",
                Style::default().fg(p.overlay0),
            )),
            Rect::new(footer.x, footer.y, footer.width, 1),
        );
    }
}

/// Text prompt for naming a new branch, sharing the picker's popup chrome.
pub(super) fn render_git_branch_create_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let p = &app.palette;
    let Some(popup) = centered_popup_rect(area, PICKER_WIDTH, 7) else {
        return;
    };

    super::dim_background(frame, area);
    let Some(inner) = render_panel_shell(frame, popup, p.accent, p.panel_bg) else {
        return;
    };
    if inner.height < 3 || inner.width < 10 {
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " new branch",
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        )])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let typed = if app.name_input.is_empty() {
        Span::styled(
            "branch name…",
            Style::default().fg(p.overlay0).add_modifier(Modifier::DIM),
        )
    } else {
        Span::styled(app.name_input.clone(), Style::default().fg(p.text))
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(" "), typed])),
        Rect::new(inner.x, inner.y + 2, inner.width, 1),
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            " enter create   esc cancel",
            Style::default().fg(p.overlay0),
        )),
        Rect::new(
            inner.x,
            inner.y + inner.height.saturating_sub(1),
            inner.width,
            1,
        ),
    );
}

fn render_body(picker: &GitPickerState, frame: &mut Frame, area: Rect, app: &AppState) {
    let p = &app.palette;
    if area.height == 0 {
        return;
    }

    let message = if picker.loading {
        Some("loading…".to_string())
    } else if let Some(error) = picker.error.as_ref() {
        Some(error.clone())
    } else if picker.entries.is_empty() {
        Some(picker.kind.empty_message().to_string())
    } else {
        None
    };

    if let Some(message) = message {
        let style = if picker.error.is_some() {
            Style::default().fg(p.red)
        } else {
            Style::default().fg(p.overlay0)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {message}"), style)),
            Rect::new(area.x, area.y, area.width, 1),
        );
        return;
    }

    // Keep the highlighted row on screen for lists longer than the panel.
    let visible = area.height as usize;
    let start = picker
        .selected
        .selected
        .saturating_sub(visible.saturating_sub(1));

    for (offset, entry) in picker.entries.iter().skip(start).take(visible).enumerate() {
        let y = area.y + offset as u16;
        let is_selected = start + offset == picker.selected.selected;
        if is_selected {
            let buf = frame.buffer_mut();
            for x in area.x..area.x + area.width {
                buf[(x, y)].set_style(Style::default().bg(p.selection_bg));
            }
        }
        let style = if is_selected {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.subtext0)
        };
        let label = truncate_end(&entry.label, area.width.saturating_sub(1) as usize);
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::raw(" "), Span::styled(label, style)])),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}
