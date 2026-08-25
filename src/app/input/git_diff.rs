use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{state::Mode, App};

impl App {
    /// Handles a key while the side-by-side diff view (`Mode::GitDiff`) is showing in place of
    /// the terminal area. `Esc`/`q` closes it and returns to the normal terminal/agent view.
    pub(crate) fn handle_git_diff_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_git_diff_view(),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_git_diff(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_git_diff(1),
            KeyCode::PageUp => self.scroll_git_diff(-10),
            KeyCode::PageDown => self.scroll_git_diff(10),
            _ => {}
        }
    }

    pub(crate) fn close_git_diff_view(&mut self) {
        self.state.git_diff_view = None;
        self.state.mode = if self.state.active.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };
    }

    fn scroll_git_diff(&mut self, delta: isize) {
        // Clamped against the geometry `compute_view` recorded: without it, over-scrolling walks
        // past the last row and paints an empty body the user must scroll all the way back from.
        let max_scroll = self.state.view.git_diff_max_scroll;
        let Some(view) = self.state.git_diff_view.as_mut() else {
            return;
        };
        view.scroll = view.scroll.saturating_add_signed(delta).min(max_scroll);
    }
}
