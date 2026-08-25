use std::path::{Path, PathBuf};

use super::App;
use crate::events::{AppEvent, GitFileAction};

impl App {
    /// Stages/unstages/discards a single file in the background, following the
    /// `git_refresh.rs`/`worktrees.rs` idiom: `std::thread::spawn` + blocking `git` call, result
    /// posted back through the event channel.
    pub(crate) fn start_git_file_action(
        &mut self,
        repo_root: PathBuf,
        path: String,
        action: GitFileAction,
    ) {
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = run_git_file_action(&repo_root, &path, action);
            let _ = event_tx.blocking_send(AppEvent::GitFileActionFinished {
                action,
                path,
                result,
            });
        });
    }

    pub(crate) fn handle_git_file_action_finished(
        &mut self,
        action: GitFileAction,
        path: String,
        result: Result<(), String>,
    ) {
        self.state.git_sidebar.last_error = result
            .as_ref()
            .err()
            .map(|err| format!("{action:?} {path} failed: {err}"));
        if result.is_ok() {
            self.state.git_sidebar.pending_discard = None;
        }
        self.force_git_working_tree_refresh_due();
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }

    pub(crate) fn start_git_commit(&mut self, repo_root: PathBuf, message: String) {
        self.state.git_sidebar.commit_in_flight = true;
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = run_git_commit(&repo_root, &message);
            let _ = event_tx.blocking_send(AppEvent::GitCommitFinished { result });
        });
    }

    pub(crate) fn handle_git_commit_finished(&mut self, result: Result<(), String>) {
        self.state.git_sidebar.commit_in_flight = false;
        match result {
            Ok(()) => {
                self.state.git_sidebar.last_error = None;
                self.state.git_sidebar.commit_message.clear();
            }
            Err(err) => {
                self.state.git_sidebar.last_error = Some(format!("commit failed: {err}"));
            }
        }
        self.force_git_working_tree_refresh_due();
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }
}

fn run_git_file_action(repo_root: &Path, path: &str, action: GitFileAction) -> Result<(), String> {
    let mut command = crate::noninteractive_process::command("git");
    command.arg("-C").arg(repo_root);
    match action {
        GitFileAction::Stage => {
            command.args(["add", "--"]).arg(path);
        }
        GitFileAction::Unstage => {
            command.args(["restore", "--staged", "--"]).arg(path);
        }
        GitFileAction::Discard => {
            command.args(["restore", "--"]).arg(path);
        }
    }
    run_git_command(command)
}

fn run_git_commit(repo_root: &Path, message: &str) -> Result<(), String> {
    let mut command = crate::noninteractive_process::command("git");
    command
        .arg("-C")
        .arg(repo_root)
        .args(["commit", "-m", message]);
    run_git_command(command)
}

fn run_git_command(mut command: std::process::Command) -> Result<(), String> {
    let output = command.output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "herdr-app-git-file-actions-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn stage_unstage_discard_and_commit_round_trip() {
        let repo = temp_test_dir("git-file-actions");
        run_git(&repo, &["init", "--quiet"]);
        run_git(&repo, &["config", "user.email", "herdr@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Herdr Test"]);
        std::fs::write(repo.join("f.txt"), "one\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "--quiet", "-m", "initial"]);

        std::fs::write(repo.join("f.txt"), "one\ntwo\n").unwrap();

        run_git_file_action(&repo, "f.txt", GitFileAction::Stage).expect("stage");
        let staged = crate::workspace::git_working_tree_status(&repo).unwrap();
        assert!(staged.staged.iter().any(|e| e.path == "f.txt"));
        assert!(!staged.unstaged.iter().any(|e| e.path == "f.txt"));

        run_git_file_action(&repo, "f.txt", GitFileAction::Unstage).expect("unstage");
        let unstaged = crate::workspace::git_working_tree_status(&repo).unwrap();
        assert!(!unstaged.staged.iter().any(|e| e.path == "f.txt"));
        assert!(unstaged.unstaged.iter().any(|e| e.path == "f.txt"));

        run_git_file_action(&repo, "f.txt", GitFileAction::Discard).expect("discard");
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "one\n"
        );

        std::fs::write(repo.join("f.txt"), "one\nstaged for commit\n").unwrap();
        run_git_file_action(&repo, "f.txt", GitFileAction::Stage).expect("stage for commit");
        run_git_commit(&repo, "second commit").expect("commit");
        let after_commit = crate::workspace::git_working_tree_status(&repo).unwrap();
        assert!(after_commit.staged.is_empty());
        assert!(after_commit.unstaged.is_empty());

        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn failing_git_command_reports_stderr() {
        let repo = temp_test_dir("git-file-actions-failure");
        run_git(&repo, &["init", "--quiet"]);

        let result = run_git_file_action(&repo, "does-not-exist.txt", GitFileAction::Stage);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(repo);
    }
}
