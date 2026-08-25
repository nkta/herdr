use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileStatusKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileEntry {
    pub path: String,
    pub original_path: Option<String>,
    pub status: GitFileStatusKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitWorkingTreeStatus {
    pub staged: Vec<GitFileEntry>,
    pub unstaged: Vec<GitFileEntry>,
}

/// Shells `git status --porcelain=v2 -z` for the working-tree file list (staged index changes
/// and unstaged/untracked worktree changes). Returns `None` when the command fails to run or
/// `repo_root` isn't a git repo — callers already know `repo_root` from `GitSpaceMetadata`, so a
/// failure here means the repo state changed out from under us, not "not a repo".
pub fn git_working_tree_status(repo_root: &Path) -> Option<GitWorkingTreeStatus> {
    let output = crate::noninteractive_process::command("git")
        .arg("-C")
        .arg(repo_root)
        .args(["status", "--porcelain=v2", "-z", "--untracked-files=all"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_porcelain_v2(&output.stdout))
}

/// Pure parser for `git status --porcelain=v2 -z` output. Records are NUL-separated; a rename/
/// copy record ("2 ...") consumes one extra NUL-delimited token for the original path.
pub(crate) fn parse_porcelain_v2(raw: &[u8]) -> GitWorkingTreeStatus {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    let mut tokens = raw.split(|&b| b == 0).peekable();
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            continue;
        }
        let record = String::from_utf8_lossy(token);
        let mut parts = record.splitn(2, ' ');
        let record_type = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");

        match record_type {
            "1" => {
                if let Some((xy, path)) = parse_ordinary_fields(rest) {
                    push_split_entry(&mut staged, &mut unstaged, xy, path.to_string(), None);
                }
            }
            "2" => {
                if let Some((xy, path)) = parse_rename_fields(rest) {
                    let original_path = tokens
                        .next()
                        .map(|t| String::from_utf8_lossy(t).into_owned());
                    push_split_entry(&mut staged, &mut unstaged, xy, path, original_path);
                }
            }
            "u" => {
                if let Some(path) = parse_unmerged_fields(rest) {
                    unstaged.push(GitFileEntry {
                        path,
                        original_path: None,
                        status: GitFileStatusKind::Conflicted,
                    });
                }
            }
            "?" => {
                unstaged.push(GitFileEntry {
                    path: rest.to_string(),
                    original_path: None,
                    status: GitFileStatusKind::Untracked,
                });
            }
            _ => {
                // "!" (ignored, not requested) or any future/unknown record type: skip.
            }
        }
    }

    GitWorkingTreeStatus { staged, unstaged }
}

/// `<XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`
fn parse_ordinary_fields(rest: &str) -> Option<(&str, &str)> {
    let mut fields = rest.splitn(8, ' ');
    let xy = fields.next()?;
    for _ in 0..6 {
        fields.next()?;
    }
    let path = fields.next()?;
    Some((xy, path))
}

/// `<XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>` (origPath follows as a separate token)
fn parse_rename_fields(rest: &str) -> Option<(&str, String)> {
    let mut fields = rest.splitn(9, ' ');
    let xy = fields.next()?;
    for _ in 0..6 {
        fields.next()?;
    }
    let _score = fields.next()?;
    let path = fields.next()?;
    Some((xy, path.to_string()))
}

/// `<XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`
fn parse_unmerged_fields(rest: &str) -> Option<String> {
    let mut fields = rest.splitn(10, ' ');
    for _ in 0..9 {
        fields.next()?;
    }
    let path = fields.next()?;
    Some(path.to_string())
}

fn push_split_entry(
    staged: &mut Vec<GitFileEntry>,
    unstaged: &mut Vec<GitFileEntry>,
    xy: &str,
    path: String,
    original_path: Option<String>,
) {
    let mut chars = xy.chars();
    let x = chars.next().unwrap_or('.');
    let y = chars.next().unwrap_or('.');
    if x != '.' {
        staged.push(GitFileEntry {
            path: path.clone(),
            original_path: original_path.clone(),
            status: status_kind_from_char(x),
        });
    }
    if y != '.' {
        unstaged.push(GitFileEntry {
            path,
            original_path,
            status: status_kind_from_char(y),
        });
    }
}

fn status_kind_from_char(c: char) -> GitFileStatusKind {
    match c {
        'A' => GitFileStatusKind::Added,
        'D' => GitFileStatusKind::Deleted,
        'R' | 'C' => GitFileStatusKind::Renamed,
        _ => GitFileStatusKind::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(records: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for record in records {
            out.extend_from_slice(record.as_bytes());
            out.push(0);
        }
        out
    }

    #[test]
    fn parses_staged_added_file() {
        let raw = joined(&["1 A. N... 000000 100644 100644 0000000000000000000000000000000000000000 abc123 new_file.rs"]);
        let status = parse_porcelain_v2(&raw);
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "new_file.rs");
        assert_eq!(status.staged[0].status, GitFileStatusKind::Added);
        assert!(status.unstaged.is_empty());
    }

    #[test]
    fn parses_unstaged_modified_file() {
        let raw = joined(&["1 .M N... 100644 100644 100644 abc123 abc123 src/lib.rs"]);
        let status = parse_porcelain_v2(&raw);
        assert!(status.staged.is_empty());
        assert_eq!(status.unstaged.len(), 1);
        assert_eq!(status.unstaged[0].path, "src/lib.rs");
        assert_eq!(status.unstaged[0].status, GitFileStatusKind::Modified);
    }

    #[test]
    fn splits_file_modified_in_both_index_and_worktree() {
        let raw = joined(&["1 MM N... 100644 100644 100644 abc123 def456 both.rs"]);
        let status = parse_porcelain_v2(&raw);
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.unstaged.len(), 1);
        assert_eq!(status.staged[0].path, "both.rs");
        assert_eq!(status.unstaged[0].path, "both.rs");
    }

    #[test]
    fn parses_untracked_file() {
        let raw = joined(&["? scratch.txt"]);
        let status = parse_porcelain_v2(&raw);
        assert_eq!(status.staged.len(), 0);
        assert_eq!(status.unstaged.len(), 1);
        assert_eq!(status.unstaged[0].status, GitFileStatusKind::Untracked);
        assert_eq!(status.unstaged[0].path, "scratch.txt");
    }

    #[test]
    fn parses_staged_rename_with_two_tokens() {
        let raw = joined(&[
            "2 R. N... 100644 100644 100644 abc123 abc123 R100 new_name.rs",
            "old_name.rs",
        ]);
        let status = parse_porcelain_v2(&raw);
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "new_name.rs");
        assert_eq!(
            status.staged[0].original_path.as_deref(),
            Some("old_name.rs")
        );
        assert_eq!(status.staged[0].status, GitFileStatusKind::Renamed);
        assert!(status.unstaged.is_empty());
    }

    #[test]
    fn parses_unmerged_conflict() {
        let raw =
            joined(&["u UU N... 100644 100644 100644 100644 abc123 def456 ghi789 conflicted.rs"]);
        let status = parse_porcelain_v2(&raw);
        assert!(status.staged.is_empty());
        assert_eq!(status.unstaged.len(), 1);
        assert_eq!(status.unstaged[0].status, GitFileStatusKind::Conflicted);
        assert_eq!(status.unstaged[0].path, "conflicted.rs");
    }

    #[test]
    fn empty_output_yields_empty_status() {
        let status = parse_porcelain_v2(&[]);
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
    }

    #[test]
    fn ignores_unknown_record_types() {
        let raw = joined(&["! ignored.log"]);
        let status = parse_porcelain_v2(&raw);
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
    }

    #[test]
    fn git_working_tree_status_reports_staged_unstaged_and_untracked() {
        use crate::workspace::git::test_support::{run_git, temp_test_dir};

        let repo = temp_test_dir("worktree-status");
        run_git(&repo, &["init", "--quiet"]);
        run_git(&repo, &["config", "user.email", "herdr@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Herdr Test"]);
        std::fs::write(repo.join("tracked.txt"), "one\n").unwrap();
        run_git(&repo, &["add", "tracked.txt"]);
        run_git(&repo, &["commit", "--quiet", "-m", "initial"]);

        std::fs::write(repo.join("tracked.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join("staged.txt"), "staged\n").unwrap();
        run_git(&repo, &["add", "staged.txt"]);
        std::fs::write(repo.join("untracked.txt"), "untracked\n").unwrap();

        let status = git_working_tree_status(&repo).expect("status should succeed");

        assert!(status
            .staged
            .iter()
            .any(|e| e.path == "staged.txt" && e.status == GitFileStatusKind::Added));
        assert!(status
            .unstaged
            .iter()
            .any(|e| e.path == "tracked.txt" && e.status == GitFileStatusKind::Modified));
        assert!(status
            .unstaged
            .iter()
            .any(|e| e.path == "untracked.txt" && e.status == GitFileStatusKind::Untracked));

        let _ = std::fs::remove_dir_all(repo);
    }
}
