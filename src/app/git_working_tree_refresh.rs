use std::path::PathBuf;
use std::time::Instant;

use super::{App, GIT_WORKING_TREE_REFRESH_INTERVAL};
use crate::app::state::{Mode, SelectionListState, SidebarSpacesView};
use crate::events::AppEvent;

impl App {
    /// Refreshes the Git sidebar's working-tree file status (staged/unstaged/untracked), gated on
    /// the Git tab actually being visible — unlike branch/ahead-behind (`git_refresh.rs`), file
    /// status is cheap to skip entirely when nobody can see it, and changes on every keystroke in
    /// the user's editor, so a debounced refetch (no mtime fingerprint cache) is the right v1
    /// tradeoff.
    pub(crate) fn start_git_working_tree_refresh_if_due(&mut self, now: Instant) {
        // Before any gating: a workspace switch must invalidate the panel immediately, not after
        // the debounce interval and not only while the Git tab happens to be visible.
        self.invalidate_git_panel_on_repo_change();

        if self.state.sidebar_spaces_view != SidebarSpacesView::Git {
            return;
        }
        if self.git_working_tree_refresh_in_flight {
            return;
        }
        if now < self.last_git_working_tree_refresh + GIT_WORKING_TREE_REFRESH_INTERVAL {
            return;
        }

        let Some(repo_root) = self.active_git_repo_root() else {
            self.state.git_sidebar.target_repo_root = None;
            self.last_git_working_tree_refresh = now;
            return;
        };

        self.state.git_sidebar.target_repo_root = Some(repo_root.clone());
        self.git_working_tree_refresh_in_flight = true;
        self.last_git_working_tree_refresh = now;

        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let status = crate::workspace::git_working_tree_status(&repo_root);
            let _ = event_tx
                .blocking_send(AppEvent::GitWorkingTreeStatusRefreshed { repo_root, status });
        });
    }

    pub(crate) fn handle_git_working_tree_status_refreshed(
        &mut self,
        repo_root: PathBuf,
        status: Option<crate::workspace::GitWorkingTreeStatus>,
    ) {
        self.git_working_tree_refresh_in_flight = false;
        // Drop results for a repo we've since navigated away from, so a slow in-flight fetch
        // can't clobber a freshly selected workspace's panel.
        if self.state.git_sidebar.target_repo_root.as_deref() != Some(repo_root.as_path()) {
            return;
        }
        self.state.git_sidebar.status = status;

        // The row list can shrink under the cursor (files committed, or `git stash` run in a
        // pane). Leaving `selected` past the end makes the panel highlight nothing and every
        // action silently inert, with `move_prev` needing one press per stale index to recover.
        let git = &mut self.state.git_sidebar;
        let row_count = git.rows().len();
        if git.selected.selected >= row_count {
            git.selected = SelectionListState::new(row_count.saturating_sub(1));
        }
        // An armed discard whose file is gone must not stay pending.
        if let Some(path) = git.pending_discard.clone() {
            let still_present = git.rows().iter().any(|(entry, _)| entry.path == path);
            if !still_present {
                git.pending_discard = None;
            }
        }

        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }

    /// Drops everything the Git panel was showing when the active workspace moves to a different
    /// repository (or to none).
    ///
    /// Without this, the panel keeps rendering the previous repository's file rows while actions
    /// resolve a repository root, so acting on a stale row could run `git restore` on a
    /// same-named path in the *new* repository and destroy uncommitted work there.
    fn invalidate_git_panel_on_repo_change(&mut self) {
        let active_repo = self.active_git_repo_root();
        if self.state.git_sidebar.target_repo_root == active_repo {
            return;
        }

        let git = &mut self.state.git_sidebar;
        git.target_repo_root = active_repo.clone();
        git.status = None;
        git.selected = SelectionListState::new(0);
        git.pending_discard = None;
        git.last_error = None;
        // The draft belongs to the repository it was written for; carrying it over would attach
        // it to an unrelated commit.
        git.commit_message.clear();

        // A diff from the old repository must not stay painted over the new workspace.
        if self
            .state
            .git_diff_view
            .as_ref()
            .is_some_and(|view| Some(&view.repo_root) != active_repo.as_ref())
        {
            self.state.git_diff_view = None;
            if self.state.mode == Mode::GitDiff {
                self.state.mode = if self.state.active.is_some() {
                    Mode::Terminal
                } else {
                    Mode::Navigate
                };
            }
        }
    }

    /// Bypasses the debounce so the next tick refetches immediately — used after a mutating
    /// stage/unstage/discard/commit action instead of waiting out the interval.
    pub(crate) fn force_git_working_tree_refresh_due(&mut self) {
        self.last_git_working_tree_refresh = Instant::now()
            .checked_sub(GIT_WORKING_TREE_REFRESH_INTERVAL)
            .unwrap_or_else(Instant::now);
    }

    pub(crate) fn active_git_repo_root(&self) -> Option<PathBuf> {
        let ws = self
            .state
            .active
            .and_then(|idx| self.state.workspaces.get(idx))?;
        ws.git_space().map(|space| space.repo_root.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

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
    fn refresh_is_not_due_when_spaces_tab_is_active() {
        let mut app = test_app();
        app.state.sidebar_spaces_view = SidebarSpacesView::Spaces;
        app.last_git_working_tree_refresh = Instant::now() - GIT_WORKING_TREE_REFRESH_INTERVAL * 2;

        app.start_git_working_tree_refresh_if_due(Instant::now());

        assert!(!app.git_working_tree_refresh_in_flight);
    }

    #[test]
    fn refresh_is_not_due_before_the_interval_elapses() {
        let mut app = test_app();
        app.state.sidebar_spaces_view = SidebarSpacesView::Git;
        app.state.workspaces.push(Workspace::test_new("test"));
        app.state.active = Some(0);
        app.last_git_working_tree_refresh = Instant::now();

        app.start_git_working_tree_refresh_if_due(Instant::now());

        assert!(!app.git_working_tree_refresh_in_flight);
    }

    #[test]
    fn refresh_skips_workspaces_without_a_git_space() {
        let mut app = test_app();
        app.state.sidebar_spaces_view = SidebarSpacesView::Git;
        app.state.workspaces.push(Workspace::test_new("test"));
        app.state.active = Some(0);
        app.last_git_working_tree_refresh = Instant::now() - GIT_WORKING_TREE_REFRESH_INTERVAL * 2;

        app.start_git_working_tree_refresh_if_due(Instant::now());

        assert!(!app.git_working_tree_refresh_in_flight);
        assert!(app.state.git_sidebar.target_repo_root.is_none());
    }

    /// Switching to a different repository must not leave the previous repository's rows on
    /// screen: acting on one of them would run `git restore` against a same-named path in the new
    /// repository and destroy uncommitted work there.
    #[test]
    fn switching_repository_clears_the_previous_panel_state() {
        use crate::app::state::{GitDiffViewState, SelectionListState};
        use crate::workspace::{GitFileEntry, GitFileStatusKind, GitWorkingTreeStatus};

        let mut app = test_app();
        let repo_a = std::path::PathBuf::from("/repo-a");
        let repo_b = std::path::PathBuf::from("/repo-b");

        // Panel is showing repo A, with a selection, a pending discard and a commit draft.
        app.state.git_sidebar.target_repo_root = Some(repo_a.clone());
        app.state.git_sidebar.status = Some(GitWorkingTreeStatus {
            staged: Vec::new(),
            unstaged: vec![GitFileEntry {
                path: "src/main.rs".into(),
                original_path: None,
                status: GitFileStatusKind::Modified,
            }],
        });
        app.state.git_sidebar.selected = SelectionListState::new(0);
        app.state.git_sidebar.pending_discard = Some("src/main.rs".into());
        app.state.git_sidebar.commit_message = "fix for repo A".into();
        app.state.git_diff_view = Some(GitDiffViewState {
            repo_root: repo_a,
            path: "src/main.rs".into(),
            staged: false,
            diff: None,
            scroll: 0,
            row_count: 0,
            loading: false,
        });

        // The active workspace now points at repo B.
        let mut workspace = Workspace::test_new("b");
        workspace.cached_git_space = Some(crate::workspace::GitSpaceMetadata {
            key: "/repo-b/.git".into(),
            checkout_key: "/repo-b".into(),
            repo_name: "repo-b".into(),
            repo_root: repo_b.clone(),
            is_linked_worktree: false,
        });
        app.state.workspaces.push(workspace);
        app.state.active = Some(0);

        app.start_git_working_tree_refresh_if_due(Instant::now());

        let git = &app.state.git_sidebar;
        assert_eq!(git.target_repo_root, Some(repo_b));
        assert!(git.status.is_none(), "repo A's rows must not survive");
        assert!(
            git.pending_discard.is_none(),
            "armed discard must be dropped"
        );
        assert!(git.commit_message.is_empty(), "draft belonged to repo A");
        assert!(
            app.state.git_diff_view.is_none(),
            "repo A's diff must not stay painted over repo B"
        );
    }

    #[test]
    fn force_refresh_makes_the_next_tick_due_immediately() {
        let mut app = test_app();
        app.last_git_working_tree_refresh = Instant::now();

        app.force_git_working_tree_refresh_due();

        assert!(
            app.last_git_working_tree_refresh <= Instant::now() - GIT_WORKING_TREE_REFRESH_INTERVAL
        );
    }
}
