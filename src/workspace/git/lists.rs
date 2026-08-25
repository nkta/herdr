use std::path::Path;

/// One selectable git object: `value` is what gets passed back to git, `label` is what the user
/// sees. They differ for stashes (`stash@{0}` vs its human message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitListEntry {
    pub value: String,
    pub label: String,
}

/// Field separator used in `--format` strings. A unit separator cannot appear in a branch name
/// (git forbids control characters) nor realistically in a stash subject, so splitting on it is
/// safer than guessing at a printable delimiter such as `:`.
const FIELD_SEP: char = '\x1f';

pub fn git_stash_list(repo_root: &Path) -> Option<Vec<GitListEntry>> {
    let output = crate::noninteractive_process::command("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "--no-pager",
            "stash",
            "list",
            &format!("--format=%gd{FIELD_SEP}%gs"),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_stash_list(&String::from_utf8_lossy(&output.stdout)))
}

pub(crate) fn parse_stash_list(raw: &str) -> Vec<GitListEntry> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let (value, subject) = line.split_once(FIELD_SEP)?;
            Some(GitListEntry {
                value: value.to_string(),
                label: format!("{value}  {subject}"),
            })
        })
        .collect()
}

pub fn git_branch_list(repo_root: &Path) -> Option<Vec<GitListEntry>> {
    let output = crate::noninteractive_process::command("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "--no-pager",
            "branch",
            "--format=%(refname:short)",
            "--sort=-committerdate",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_branch_list(&String::from_utf8_lossy(&output.stdout)))
}

pub(crate) fn parse_branch_list(raw: &str) -> Vec<GitListEntry> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        // A detached HEAD is reported as "(HEAD detached at abc1234)", which is not switchable.
        .filter(|line| !line.starts_with('('))
        .map(|line| GitListEntry {
            value: line.to_string(),
            label: line.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stash_entries_into_value_and_label() {
        let raw = "stash@{0}\x1fWIP on master: 1a2b3c fix parser\n\
                   stash@{1}\x1fOn feature: experiment\n";
        let entries = parse_stash_list(raw);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].value, "stash@{0}");
        assert!(entries[0].label.contains("fix parser"));
        assert_eq!(entries[1].value, "stash@{1}");
    }

    #[test]
    fn stash_subject_containing_a_colon_stays_intact() {
        // A ':' delimiter would have split this subject in the wrong place.
        let entries = parse_stash_list("stash@{0}\x1fWIP on main: refactor: split module\n");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, "stash@{0}");
        assert!(entries[0].label.contains("refactor: split module"));
    }

    #[test]
    fn empty_stash_list_yields_no_entries() {
        assert!(parse_stash_list("").is_empty());
    }

    #[test]
    fn parses_branch_names_and_skips_detached_head() {
        let entries = parse_branch_list("main\nfeature/login\n(HEAD detached at 1a2b3c)\n");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].value, "main");
        assert_eq!(entries[1].value, "feature/login");
    }

    #[test]
    fn stash_and_branch_listing_round_trip_against_a_real_repo() {
        use crate::workspace::git::test_support::{run_git, temp_test_dir};

        let repo = temp_test_dir("git-lists");
        run_git(&repo, &["init", "--quiet"]);
        run_git(&repo, &["config", "user.email", "herdr@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Herdr Test"]);
        std::fs::write(repo.join("f.txt"), "one\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "--quiet", "-m", "initial"]);
        run_git(&repo, &["branch", "feature/x"]);

        let branches = git_branch_list(&repo).expect("branch list");
        assert!(branches.iter().any(|entry| entry.value == "feature/x"));

        assert!(
            git_stash_list(&repo).expect("stash list").is_empty(),
            "a fresh repo has no stashes"
        );

        std::fs::write(repo.join("f.txt"), "one\ntwo\n").unwrap();
        run_git(&repo, &["stash", "push", "-m", "work in progress"]);
        let stashes = git_stash_list(&repo).expect("stash list");
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].value, "stash@{0}");
        assert!(stashes[0].label.contains("work in progress"));

        let _ = std::fs::remove_dir_all(repo);
    }
}
