use std::path::PathBuf;

use super::App;
use crate::app::state::{GitDiffViewState, GitSidebarFocus, Mode};
use crate::events::AppEvent;
use crate::workspace::{git_file_diff, git_untracked_file_diff, FileDiff};

impl App {
    /// Opens the side-by-side diff view for one file and kicks off the background fetch —
    /// shared by the keyboard (`Enter` on the selected row) and mouse (row click) paths.
    pub(crate) fn open_git_diff(
        &mut self,
        repo_root: PathBuf,
        path: String,
        staged: bool,
        untracked: bool,
    ) {
        // Move the sidebar's selection cursor onto the file being opened, so a mouse click gets
        // the same row highlight the keyboard path already had. Without this the cursor stays
        // wherever it was and the clicked row shows no feedback at all.
        if let Some(index) = self
            .state
            .git_sidebar
            .rows()
            .iter()
            .position(|(entry, row_staged)| entry.path == path && *row_staged == staged)
        {
            self.state.git_sidebar.selected.select(index);
        }

        self.state.git_diff_view = Some(GitDiffViewState {
            repo_root: repo_root.clone(),
            path: path.clone(),
            staged,
            diff: None,
            scroll: 0,
            row_count: 0,
            loading: true,
        });
        // Keyboard focus stays on the file list rather than jumping into the diff: opening a diff
        // is usually a step toward staging, so `s`/`u`/`d` and the arrows must remain live.
        // `Mode::GitDiff` is entered by clicking inside the diff panel, when scrolling it is what
        // the user actually wants.
        self.state.mode = Mode::SidebarGit;
        self.state.git_sidebar.focus = GitSidebarFocus::FileList;
        self.start_git_diff_fetch(repo_root, path, staged, untracked);
    }

    /// Fetches the diff for a single file, one-shot (not polled). `untracked`: read the file
    /// directly and synthesize an all-additions diff instead of shelling `git diff --no-index`
    /// (see `workspace::git::diff::git_untracked_file_diff` for why). Superseded fetches are
    /// dropped by generation rather than cancelled — `AppEvent::GitDiffReady` carries the
    /// generation it was launched with, and stale results are ignored on arrival.
    pub(crate) fn start_git_diff_fetch(
        &mut self,
        repo_root: PathBuf,
        path: String,
        staged: bool,
        untracked: bool,
    ) {
        self.git_diff_fetch_generation += 1;
        let generation = self.git_diff_fetch_generation;
        let event_tx = self.event_tx.clone();

        std::thread::spawn(move || {
            let diff = if untracked {
                Some(git_untracked_file_diff(&repo_root, &path))
            } else {
                git_file_diff(&repo_root, &path, staged)
            };
            let _ = event_tx.blocking_send(AppEvent::GitDiffReady {
                generation,
                repo_root,
                path,
                staged,
                diff,
            });
        });
    }

    pub(crate) fn handle_git_diff_ready(
        &mut self,
        generation: u64,
        repo_root: PathBuf,
        path: String,
        staged: bool,
        diff: Option<FileDiff>,
    ) {
        if generation != self.git_diff_fetch_generation {
            return;
        }
        let Some(view) = self.state.git_diff_view.as_mut() else {
            return;
        };
        if view.repo_root != repo_root || view.path != path || view.staged != staged {
            return;
        }
        view.row_count = diff
            .as_ref()
            .map(|diff| {
                diff.hunks
                    .iter()
                    .map(|hunk| crate::workspace::hunk_to_side_by_side(hunk).len())
                    .sum()
            })
            .unwrap_or(0);
        view.diff = diff;
        view.loading = false;
        view.scroll = 0;
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::GitDiffViewState;

    fn test_app() -> App {
        App::new(
            &crate::config::Config::default(),
            true,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        )
    }

    #[test]
    fn stale_generation_is_ignored() {
        let mut app = test_app();
        app.state.git_diff_view = Some(GitDiffViewState {
            repo_root: PathBuf::from("/repo"),
            path: "f.rs".into(),
            staged: false,
            diff: None,
            scroll: 0,
            row_count: 0,
            loading: true,
        });
        app.git_diff_fetch_generation = 5;

        app.handle_git_diff_ready(
            3,
            PathBuf::from("/repo"),
            "f.rs".into(),
            false,
            Some(FileDiff::default()),
        );

        assert!(app.state.git_diff_view.as_ref().unwrap().diff.is_none());
        assert!(app.state.git_diff_view.as_ref().unwrap().loading);
    }

    #[test]
    fn matching_generation_and_selection_applies_diff() {
        let mut app = test_app();
        app.state.git_diff_view = Some(GitDiffViewState {
            repo_root: PathBuf::from("/repo"),
            path: "f.rs".into(),
            staged: false,
            diff: None,
            scroll: 0,
            row_count: 0,
            loading: true,
        });
        app.git_diff_fetch_generation = 1;

        app.handle_git_diff_ready(
            1,
            PathBuf::from("/repo"),
            "f.rs".into(),
            false,
            Some(FileDiff::default()),
        );

        let view = app.state.git_diff_view.as_ref().unwrap();
        assert!(view.diff.is_some());
        assert!(!view.loading);
    }

    #[test]
    fn result_for_a_different_selection_is_ignored() {
        let mut app = test_app();
        app.state.git_diff_view = Some(GitDiffViewState {
            repo_root: PathBuf::from("/repo"),
            path: "other.rs".into(),
            staged: false,
            diff: None,
            scroll: 0,
            row_count: 0,
            loading: true,
        });
        app.git_diff_fetch_generation = 1;

        app.handle_git_diff_ready(
            1,
            PathBuf::from("/repo"),
            "f.rs".into(),
            false,
            Some(FileDiff::default()),
        );

        assert!(app.state.git_diff_view.as_ref().unwrap().diff.is_none());
    }
}
