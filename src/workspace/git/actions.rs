use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileActionKind {
    Stage,
    Unstage,
    Discard,
}

pub fn run_git_file_action(
    repo_root: &Path,
    path: &str,
    action: GitFileActionKind,
) -> Result<(), String> {
    let mut command = crate::noninteractive_process::command("git");
    command.arg("-C").arg(repo_root);
    match action {
        GitFileActionKind::Stage => {
            command.args(["add", "--"]).arg(path);
        }
        GitFileActionKind::Unstage => {
            command.args(["restore", "--staged", "--"]).arg(path);
        }
        GitFileActionKind::Discard => {
            command.args(["restore", "--"]).arg(path);
        }
    }
    run_git_command(command)
}

pub fn run_git_commit(repo_root: &Path, message: &str) -> Result<(), String> {
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
    use crate::workspace::git::test_support::{run_git, temp_test_dir};

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

        run_git_file_action(&repo, "f.txt", GitFileActionKind::Stage).expect("stage");
        let staged = crate::workspace::git_working_tree_status(&repo).unwrap();
        assert!(staged.staged.iter().any(|e| e.path == "f.txt"));
        assert!(!staged.unstaged.iter().any(|e| e.path == "f.txt"));

        run_git_file_action(&repo, "f.txt", GitFileActionKind::Unstage).expect("unstage");
        let unstaged = crate::workspace::git_working_tree_status(&repo).unwrap();
        assert!(!unstaged.staged.iter().any(|e| e.path == "f.txt"));
        assert!(unstaged.unstaged.iter().any(|e| e.path == "f.txt"));

        run_git_file_action(&repo, "f.txt", GitFileActionKind::Discard).expect("discard");
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "one\n"
        );

        std::fs::write(repo.join("f.txt"), "one\nstaged for commit\n").unwrap();
        run_git_file_action(&repo, "f.txt", GitFileActionKind::Stage).expect("stage for commit");
        run_git_commit(&repo, "second commit").expect("commit");
        let after_commit = crate::workspace::git_working_tree_status(&repo).unwrap();
        assert!(after_commit.staged.is_empty());
        assert!(after_commit.unstaged.is_empty());

        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn failing_git_command_reports_stderr() {
        let repo = temp_test_dir("git-file-actions-failure");
        run_git(&repo, &["init", "--quiet"]);

        let result = run_git_file_action(&repo, "does-not-exist.txt", GitFileActionKind::Stage);
        assert!(result.is_err());

        std::fs::remove_dir_all(repo).unwrap();
    }
}
