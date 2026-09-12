use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use super::{App, GIT_WORKING_TREE_REFRESH_INTERVAL};
use crate::events::AppEvent;
use crate::workspace::GitWorkingTreeStatus;

impl App {
    /// Makes the next headless loop tick due for a working-tree refresh, e.g. right after a
    /// file mutation so the panel doesn't wait a full `GIT_WORKING_TREE_REFRESH_INTERVAL`.
    pub(crate) fn force_git_working_tree_refresh_now(&mut self) {
        self.last_git_working_tree_refresh = Instant::now() - GIT_WORKING_TREE_REFRESH_INTERVAL;
        self.render_notify.notify_one();
    }

    /// Sets the aggregated "at least one client is watching this workspace's git panel" fact.
    /// Returns whether it actually changed, so callers can skip a redundant repaint.
    pub(crate) fn set_git_panel_demand(&mut self, workspace_id: &str, demand: bool) -> bool {
        let Some(workspace) = self
            .state
            .workspaces
            .iter_mut()
            .find(|ws| ws.id == workspace_id)
        else {
            return false;
        };
        if workspace.git_panel_demand == demand {
            return false;
        }
        workspace.git_panel_demand = demand;
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
        true
    }

    pub(crate) fn git_working_tree_refresh_deadline(&self) -> Option<Instant> {
        (!self.git_working_tree_refresh_in_flight && self.any_workspace_wants_git_panel())
            .then_some(self.last_git_working_tree_refresh + GIT_WORKING_TREE_REFRESH_INTERVAL)
    }

    pub(crate) fn start_git_working_tree_refresh_if_due(&mut self, now: Instant) {
        let Some(deadline) = self.git_working_tree_refresh_deadline() else {
            return;
        };
        if now < deadline {
            return;
        }

        let targets = self.git_working_tree_refresh_targets();
        self.last_git_working_tree_refresh = now;
        if targets.is_empty() {
            return;
        }

        self.git_working_tree_refresh_in_flight = true;
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let updates = refresh_git_working_trees(targets);
            let _ = event_tx.blocking_send(AppEvent::GitWorkingTreeRefreshed { updates });
        });
    }

    fn any_workspace_wants_git_panel(&self) -> bool {
        self.state.workspaces.iter().any(|ws| ws.git_panel_demand)
    }

    fn git_working_tree_refresh_targets(&self) -> Vec<(String, PathBuf)> {
        self.state
            .workspaces
            .iter()
            .filter(|ws| ws.git_panel_demand)
            .filter_map(|ws| Some((ws.id.clone(), ws.git_space()?.repo_root.clone())))
            .collect()
    }
}

/// Runs `git status` once per distinct repo root, fanning the result out to every workspace
/// that shares it (e.g. linked worktrees of the same repo watched at once).
fn refresh_git_working_trees(
    targets: Vec<(String, PathBuf)>,
) -> Vec<(String, Option<GitWorkingTreeStatus>)> {
    let mut by_root: HashMap<PathBuf, Option<GitWorkingTreeStatus>> = HashMap::new();
    targets
        .into_iter()
        .map(|(workspace_id, repo_root)| {
            let status = by_root
                .entry(repo_root.clone())
                .or_insert_with(|| crate::workspace::git_working_tree_status(&repo_root))
                .clone();
            (workspace_id, status)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    fn test_app() -> App {
        App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        )
    }

    #[test]
    fn deadline_is_none_without_any_demand() {
        let mut app = test_app();
        app.state.workspaces.push(Workspace::test_new("test"));

        assert_eq!(app.git_working_tree_refresh_deadline(), None);
    }

    #[test]
    fn deadline_is_set_once_a_workspace_demands_the_panel() {
        let mut app = test_app();
        let mut ws = Workspace::test_new("test");
        ws.git_panel_demand = true;
        app.state.workspaces.push(ws);

        assert!(app.git_working_tree_refresh_deadline().is_some());
    }

    #[test]
    fn refresh_skips_workspaces_without_a_git_repo() {
        let mut app = test_app();
        let mut ws = Workspace::test_new("test");
        ws.git_panel_demand = true;
        app.state.workspaces.push(ws);
        let now = Instant::now();
        app.last_git_working_tree_refresh = now - GIT_WORKING_TREE_REFRESH_INTERVAL;

        app.start_git_working_tree_refresh_if_due(now);

        assert!(!app.git_working_tree_refresh_in_flight);
        assert!(app.event_rx.try_recv().is_err());
    }

    #[test]
    fn refresh_reports_staged_and_unstaged_files_for_a_real_repo() {
        let repo = std::env::temp_dir().join(format!(
            "herdr-git-working-tree-refresh-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.email", "herdr@example.invalid"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.name", "Herdr Test"])
            .output()
            .unwrap();
        std::fs::write(repo.join("tracked.txt"), "one\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "tracked.txt"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "--quiet", "-m", "initial"])
            .output()
            .unwrap();
        std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();

        let updates = refresh_git_working_trees(vec![("ws1".into(), repo.clone())]);

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, "ws1");
        let status = updates[0].1.as_ref().expect("status for a real repo");
        assert!(status
            .unstaged
            .iter()
            .any(|entry| entry.path == "untracked.txt"));

        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn refresh_deduplicates_targets_sharing_a_repo_root() {
        let repo = std::env::temp_dir().join(format!(
            "herdr-git-working-tree-refresh-dedupe-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "--quiet"])
            .output()
            .unwrap();

        let updates = refresh_git_working_trees(vec![
            ("ws1".into(), repo.clone()),
            ("ws2".into(), repo.clone()),
        ]);

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].1, updates[1].1);

        std::fs::remove_dir_all(repo).unwrap();
    }
}
