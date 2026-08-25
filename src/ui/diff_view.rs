use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::state::{GitDiffViewState, Palette};
use crate::app::AppState;
use crate::workspace::{hunk_to_side_by_side, SideBySideCell};

use super::widgets::render_panel_shell;

/// Width of the "esc/q close" affordance in the header row, shared with `compute_view` so the
/// clickable area matches the drawn label.
pub(crate) const CLOSE_LABEL_WIDTH: u16 = 11;

/// Side-by-side diff view that replaces the terminal area while `app.git_diff_view` is set (see
/// `ui.rs`'s `render_with_runtime_registry`) — left column is HEAD/index, right column is
/// staged/working-tree content, with synced scrolling (one `scroll` offset drives both columns).
pub(super) fn render_diff_view(
    app: &AppState,
    view: &GitDiffViewState,
    frame: &mut Frame,
    area: Rect,
) {
    let p = &app.palette;
    let Some(inner) = render_panel_shell(frame, area, p.accent, p.panel_bg) else {
        return;
    };
    if inner.height < 3 || inner.width < 10 {
        return;
    }

    let header_row = Rect::new(inner.x, inner.y, inner.width, 1);
    let left_label = if view.staged { "HEAD" } else { "index" };
    let right_label = if view.staged {
        "staged"
    } else {
        "working tree"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!(" {}  ({left_label} \u{2192} {right_label})", view.path),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        )])),
        header_row,
    );
    frame.render_widget(
        Paragraph::new(Span::styled("esc/q close", Style::default().fg(p.overlay0)))
            .alignment(Alignment::Right),
        header_row,
    );

    let body_area = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    if view.loading {
        render_placeholder(frame, p, body_area, "loading diff\u{2026}");
        return;
    }
    let Some(diff) = view.diff.as_ref() else {
        render_placeholder(frame, p, body_area, "no diff available");
        return;
    };
    if diff.binary {
        render_placeholder(frame, p, body_area, "binary file \u{2014} no preview");
        return;
    }

    let columns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body_area);
    let left_col = columns[0];
    let right_col = columns[1];

    let rows: Vec<_> = diff.hunks.iter().flat_map(hunk_to_side_by_side).collect();
    let visible = body_area.height as usize;
    for (offset, row) in rows.iter().skip(view.scroll).take(visible).enumerate() {
        let y = left_col.y + offset as u16;
        render_diff_cell(
            frame,
            p,
            Rect::new(left_col.x, y, left_col.width, 1),
            row.left.as_ref(),
            p.red,
        );
        render_diff_cell(
            frame,
            p,
            Rect::new(right_col.x, y, right_col.width, 1),
            row.right.as_ref(),
            p.green,
        );
    }
}

fn render_placeholder(frame: &mut Frame, p: &Palette, area: Rect, text: &str) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(p.overlay0))),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

/// Scales an accent color down to a muted background tint.
///
/// The palette's `green`/`red` are foreground-weight colors: used raw as a full-row background
/// they overpower the text on them. Non-RGB colors (indexed/named terminal colors) can't be
/// scaled arithmetically, so they are left untouched.
fn muted_diff_background(color: Color) -> Color {
    const SHADE: u16 = 30; // percent of the original channel value
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (u16::from(r) * SHADE / 100) as u8,
            (u16::from(g) * SHADE / 100) as u8,
            (u16::from(b) * SHADE / 100) as u8,
        ),
        other => other,
    }
}

fn render_diff_cell(
    frame: &mut Frame,
    p: &Palette,
    area: Rect,
    cell: Option<&SideBySideCell>,
    changed_bg: Color,
) {
    let Some(cell) = cell else {
        return;
    };
    if cell.changed {
        let bg = muted_diff_background(changed_bg);
        let buf = frame.buffer_mut();
        for x in area.x..area.x + area.width {
            buf[(x, area.y)].set_style(Style::default().bg(bg));
        }
    }
    frame.render_widget(
        Paragraph::new(cell.text.clone()).style(Style::default().fg(p.text)),
        area,
    );
}
