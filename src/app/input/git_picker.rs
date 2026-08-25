use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

impl App {
    /// Handles a key while the Git stash/branch picker is open.
    pub(crate) fn handle_git_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_git_picker(),
            KeyCode::Enter => self.confirm_git_picker(),
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(picker) = self.state.git_picker.as_mut() {
                    picker.selected.move_prev();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(picker) = self.state.git_picker.as_mut() {
                    let count = picker.entries.len();
                    picker.selected.move_next(count);
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// Handles a key while the "new branch" prompt is open.
    pub(crate) fn handle_git_branch_create_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_git_branch_create(),
            KeyCode::Enter => self.confirm_git_branch_create(),
            KeyCode::Backspace => {
                self.state.name_input.pop();
            }
            KeyCode::Char(c)
                if key
                    .modifiers
                    .difference(crossterm::event::KeyModifiers::SHIFT)
                    .is_empty() =>
            {
                self.state.name_input.push(c);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::state::{GitPickerKind, GitPickerState, Mode, SelectionListState};
    use crate::app::App;
    use crate::workspace::GitListEntry;
    use crossterm::event::{KeyCode, KeyEvent};

    fn app_with_picker() -> App {
        let mut app = App::new(
            &crate::config::Config::default(),
            true,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        app.state.git_picker = Some(GitPickerState {
            kind: GitPickerKind::ApplyStash,
            generation: 1,
            repo_root: "/repo".into(),
            entries: vec![
                GitListEntry {
                    value: "stash@{0}".into(),
                    label: "stash@{0}  newest".into(),
                },
                GitListEntry {
                    value: "stash@{1}".into(),
                    label: "stash@{1}  older".into(),
                },
            ],
            selected: SelectionListState::new(0),
            loading: false,
            error: None,
        });
        app.state.mode = Mode::GitPicker;
        app
    }

    #[test]
    fn arrows_move_the_selection_within_bounds() {
        let mut app = app_with_picker();

        app.handle_git_picker_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(app.state.git_picker.as_ref().unwrap().selected.selected, 1);

        // Already at the last entry: must not run past the end.
        app.handle_git_picker_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(app.state.git_picker.as_ref().unwrap().selected.selected, 1);

        app.handle_git_picker_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(app.state.git_picker.as_ref().unwrap().selected.selected, 0);
    }

    #[test]
    fn esc_closes_without_running_anything() {
        let mut app = app_with_picker();

        app.handle_git_picker_key(KeyEvent::from(KeyCode::Esc));

        assert!(app.state.git_picker.is_none());
        assert!(app.state.popup_pane.is_none());
        assert_ne!(app.state.mode, Mode::GitPicker);
    }

    #[test]
    fn stale_listing_for_another_repository_is_ignored() {
        let mut app = app_with_picker();
        if let Some(picker) = app.state.git_picker.as_mut() {
            picker.loading = true;
        }

        app.handle_git_picker_entries_ready(
            1,
            "/other-repo".into(),
            Some(vec![GitListEntry {
                value: "stash@{9}".into(),
                label: "from elsewhere".into(),
            }]),
        );

        let picker = app.state.git_picker.as_ref().unwrap();
        assert_eq!(picker.entries.len(), 2, "entries must not be replaced");
        assert_eq!(picker.entries[0].value, "stash@{0}");
        assert!(
            picker.loading,
            "a listing for another repo must not resolve this picker's loading state"
        );
    }

    /// Two pickers over the same repository (branches, then stashes) must not cross-fill: the
    /// branch listing arriving late would otherwise populate the stash picker and let Enter run
    /// `git stash pop <branch-name>`.
    #[test]
    fn listing_from_a_superseded_picker_is_rejected() {
        let mut app = app_with_picker();
        if let Some(picker) = app.state.git_picker.as_mut() {
            picker.generation = 7;
            picker.loading = true;
        }

        // A listing from the previous picker instance, same repository.
        app.handle_git_picker_entries_ready(
            6,
            "/repo".into(),
            Some(vec![GitListEntry {
                value: "main".into(),
                label: "main".into(),
            }]),
        );

        let picker = app.state.git_picker.as_ref().unwrap();
        assert_eq!(picker.entries.len(), 2, "stash entries must survive");
        assert_eq!(picker.entries[0].value, "stash@{0}");
        assert!(picker.loading, "the superseded listing must not resolve it");
    }

    #[test]
    fn branch_prompt_collects_a_name_and_cancels_cleanly() {
        let mut app = app_with_picker();
        app.state.git_picker = None;
        app.state.git_sidebar.target_repo_root = Some("/repo".into());
        app.open_git_branch_create();
        assert_eq!(app.state.mode, Mode::GitBranchCreate);

        for ch in "feat/x".chars() {
            app.handle_git_branch_create_key(KeyEvent::from(KeyCode::Char(ch)));
        }
        assert_eq!(app.state.name_input, "feat/x");

        app.handle_git_branch_create_key(KeyEvent::from(KeyCode::Backspace));
        assert_eq!(app.state.name_input, "feat/");

        app.handle_git_branch_create_key(KeyEvent::from(KeyCode::Esc));
        assert_ne!(app.state.mode, Mode::GitBranchCreate);
        assert!(
            app.state.name_input.is_empty(),
            "cancelling must not leave the typed name behind for the next dialog"
        );
        assert!(app.state.popup_pane.is_none(), "esc must not run anything");
    }

    #[test]
    fn empty_branch_name_runs_nothing() {
        let mut app = app_with_picker();
        app.state.git_picker = None;
        app.state.git_sidebar.target_repo_root = Some("/repo".into());
        app.open_git_branch_create();

        app.handle_git_branch_create_key(KeyEvent::from(KeyCode::Enter));

        assert!(app.state.popup_pane.is_none());
        assert_ne!(app.state.mode, Mode::GitBranchCreate);
    }

    #[test]
    fn failed_listing_surfaces_an_error() {
        let mut app = app_with_picker();

        app.handle_git_picker_entries_ready(1, "/repo".into(), None);

        let picker = app.state.git_picker.as_ref().unwrap();
        assert!(!picker.loading);
        assert!(picker.error.is_some());
    }
}
