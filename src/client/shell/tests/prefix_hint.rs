use super::*;

fn prefix_hint_state() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

fn frame_text(frame: &crate::protocol::FrameData) -> String {
    frame
        .cells
        .chunks(usize::from(frame.width))
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn prefix_hint_appears_only_after_the_delay() {
    let mut state = prefix_hint_state();
    let delay = state
        .config
        .prefix_hint_delay
        .expect("panel enabled by default");
    let start = std::time::Instant::now();

    state.mode = ClientShellMode::Prefix;
    assert!(!state.tick_prefix_hint(start));
    assert!(!state.prefix_hint_visible);
    assert_eq!(state.prefix_hint_deadline, Some(start + delay));

    // A second pass before the deadline must not re-arm it.
    assert!(!state.tick_prefix_hint(start + delay / 2));
    assert_eq!(state.prefix_hint_deadline, Some(start + delay));

    assert!(state.tick_prefix_hint(start + delay));
    assert!(state.prefix_hint_visible);
    assert_eq!(state.prefix_hint_deadline, None);
    // Already visible: nothing left to change.
    assert!(!state.tick_prefix_hint(start + delay * 2));
}

#[test]
fn prefix_hint_clears_when_prefix_mode_is_left() {
    let mut state = prefix_hint_state();
    let delay = state
        .config
        .prefix_hint_delay
        .expect("panel enabled by default");
    let start = std::time::Instant::now();

    state.mode = ClientShellMode::Prefix;
    state.tick_prefix_hint(start);
    assert!(state.tick_prefix_hint(start + delay));

    state.mode = ClientShellMode::Terminal;
    assert!(state.tick_prefix_hint(start + delay));
    assert!(!state.prefix_hint_visible);
    assert_eq!(state.prefix_hint_deadline, None);

    // Re-entering prefix mode starts the delay over.
    state.mode = ClientShellMode::Prefix;
    let reentry = start + delay * 3;
    assert!(!state.tick_prefix_hint(reentry));
    assert_eq!(state.prefix_hint_deadline, Some(reentry + delay));
}

#[test]
fn prefix_hint_stays_hidden_when_disabled() {
    let mut state = prefix_hint_state();
    state.config.prefix_hint_delay = None;
    let now = std::time::Instant::now();

    state.mode = ClientShellMode::Prefix;
    assert!(!state.tick_prefix_hint(now));
    assert!(!state.prefix_hint_visible);
    assert_eq!(state.prefix_hint_deadline, None);
    assert!(!state.tick_prefix_hint(now + std::time::Duration::from_secs(60)));
    assert!(!state.prefix_hint_visible);
}

#[test]
fn prefix_hint_deadline_wakes_the_loop() {
    let mut state = prefix_hint_state();
    let delay = state
        .config
        .prefix_hint_delay
        .expect("panel enabled by default");
    let now = std::time::Instant::now();

    state.mode = ClientShellMode::Prefix;
    state.tick_prefix_hint(now);

    assert!(state.timer_delay(now) <= delay);
}

#[test]
fn prefix_hint_panel_lists_grouped_prefix_keybindings() {
    let mut state = prefix_hint_state();
    state.mode = ClientShellMode::Prefix;
    state.prefix_hint_visible = true;

    let frame = state.compose(120, 30).expect("composed frame");
    let rendered = frame_text(&frame);

    assert!(rendered.contains("PREFIX"), "{rendered}");
    assert!(rendered.contains("panes"), "{rendered}");
    // The prefix is dropped: the panel is only shown once it is held.
    assert!(rendered.contains("split vertical"), "{rendered}");
    assert!(!rendered.contains("prefix+"), "{rendered}");
    // The way out of the panel stays on screen.
    assert!(rendered.contains("esc cancel"), "{rendered}");
}

#[test]
fn prefix_hint_panel_is_hidden_until_the_delay_elapses() {
    let mut state = prefix_hint_state();
    state.mode = ClientShellMode::Prefix;
    state.prefix_hint_visible = false;

    let frame = state.compose(120, 30).expect("composed frame");
    let rendered = frame_text(&frame);

    assert!(!rendered.contains("split vertical"), "{rendered}");
}

#[test]
fn prefix_hint_panel_is_skipped_when_it_cannot_fit() {
    let mut state = prefix_hint_state();
    state.mode = ClientShellMode::Prefix;
    state.prefix_hint_visible = true;

    let frame = state.compose(40, 6).expect("composed frame");
    let rendered = frame_text(&frame);

    assert!(!rendered.contains("split vertical"), "{rendered}");
}
