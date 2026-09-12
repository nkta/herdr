use super::*;

pub(super) fn render_git_picker_overlay(
    b: &mut Buffer,
    picker: &ClientGitPickerOverlay,
    p: &Palette,
) -> Option<OverlayRender> {
    let outer = popup(b.area, 50, 16)?;
    let inner = panel(b, outer, p.accent, p.panel_bg)?;
    if inner.width < 10 || inner.height < 4 {
        return None;
    }
    put_text(
        b,
        inner.x,
        inner.y,
        inner.width,
        picker.purpose.title(),
        Style::default()
            .fg(p.text)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
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
    if picker.loading {
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
    if let Some(error) = &picker.error {
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
    if picker.entries.is_empty() {
        put_text(
            b,
            body.x,
            body.y,
            body.width,
            " nothing to choose from",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        return Some(OverlayRender::default());
    }
    // Keyboard-only for now (up/down/enter/esc, see route_git_picker_overlay_key): rows aren't
    // registered as mouse hit-targets yet.
    for (index, entry) in picker.entries.iter().enumerate().take(body.height as usize) {
        let y = body.y + index as u16;
        let rect = Rect::new(body.x, y, body.width, 1);
        let selected = index == picker.selected;
        let bg = if selected { p.selection_bg } else { p.panel_bg };
        b.set_style(rect, Style::default().bg(bg));
        put_text(
            b,
            rect.x,
            rect.y,
            rect.width,
            &format!(" {}", entry.label),
            Style::default().fg(p.text).bg(bg),
        );
    }
    Some(OverlayRender::default())
}
