use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub hunks: Vec<DiffHunk>,
    pub binary: bool,
}

/// Pure parser of `git diff --no-color -U3` output for a single file: hunk headers
/// (`@@ -old_start,old_count +new_start,new_count @@ ...`) plus ' '/'+'/'-'-prefixed body
/// lines. Preamble lines (`diff --git`, `index ...`, `--- a/...`, `+++ b/...`) are skipped by
/// virtue of not being inside a hunk yet.
pub fn parse_unified_diff(diff_text: &str) -> FileDiff {
    if diff_text.contains("Binary files ") && diff_text.contains(" differ") {
        return FileDiff {
            hunks: Vec::new(),
            binary: true,
        };
    }

    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for line in diff_text.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            if let Some((old_start, new_start)) = parse_hunk_header(header) {
                old_line = old_start;
                new_line = new_start;
                current = Some(DiffHunk {
                    old_start,
                    new_start,
                    lines: Vec::new(),
                });
            }
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };

        if line.starts_with("\\ No newline") {
            continue;
        }

        if let Some(rest) = line.strip_prefix('+') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Addition,
                old_lineno: None,
                new_lineno: Some(new_line),
                text: rest.to_string(),
            });
            new_line += 1;
        } else if let Some(rest) = line.strip_prefix('-') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Deletion,
                old_lineno: Some(old_line),
                new_lineno: None,
                text: rest.to_string(),
            });
            old_line += 1;
        } else {
            let rest = line.strip_prefix(' ').unwrap_or(line);
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(old_line),
                new_lineno: Some(new_line),
                text: rest.to_string(),
            });
            old_line += 1;
            new_line += 1;
        }
    }

    if let Some(hunk) = current.take() {
        hunks.push(hunk);
    }

    FileDiff {
        hunks,
        binary: false,
    }
}

/// `header` is the hunk header text after the leading `"@@ "`, e.g. `"-10,7 +10,8 @@ fn f() {"`.
fn parse_hunk_header(header: &str) -> Option<(u32, u32)> {
    let end = header.find(" @@")?;
    let mut ranges = header[..end].split_whitespace();
    let old = ranges.next()?.strip_prefix('-')?;
    let new = ranges.next()?.strip_prefix('+')?;
    let old_start = old.split(',').next()?.parse().ok()?;
    let new_start = new.split(',').next()?.parse().ok()?;
    Some((old_start, new_start))
}

/// Fetches the diff for a tracked path. `staged`: `true` compares HEAD vs the index
/// (`git diff --cached`, i.e. left = HEAD, right = staged content); `false` compares the index
/// vs the worktree (`git diff`, i.e. left = index, right = working-tree content).
pub fn git_file_diff(repo_root: &Path, path: &str, staged: bool) -> Option<FileDiff> {
    let mut command = crate::noninteractive_process::command("git");
    command
        .arg("-C")
        .arg(repo_root)
        .args(["diff", "--no-color", "-U3"]);
    if staged {
        command.arg("--cached");
    }
    command.arg("--").arg(path);

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(parse_unified_diff(&String::from_utf8_lossy(&output.stdout)))
}

/// Synthesizes a diff for an untracked file by reading it directly rather than shelling
/// `git diff --no-index -- /dev/null <path>` (avoids the `/dev/null` vs `NUL` split between Unix
/// and the Windows target herdr ships for a case that's otherwise pure subprocess overhead, since
/// the bytes are already on disk). Every line renders as an addition; non-UTF8 content is
/// reported as binary rather than lossily reinterpreted.
pub fn git_untracked_file_diff(repo_root: &Path, path: &str) -> FileDiff {
    let file_path = repo_root.join(path);

    // `--untracked-files=all` happily lists multi-hundred-megabyte logs, core dumps and datasets.
    // Reading one whole would allocate it several times over and pin it in app state, so anything
    // past the cap is reported as unpreviewable rather than loaded.
    const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
    let too_large = std::fs::metadata(&file_path)
        .map(|meta| meta.len() > MAX_PREVIEW_BYTES)
        .unwrap_or(false);
    if too_large {
        return FileDiff {
            hunks: Vec::new(),
            binary: true,
        };
    }

    let bytes = match std::fs::read(&file_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return FileDiff {
                hunks: Vec::new(),
                binary: true,
            }
        }
    };

    let Ok(text) = String::from_utf8(bytes) else {
        return FileDiff {
            hunks: Vec::new(),
            binary: true,
        };
    };

    let lines = text
        .lines()
        .enumerate()
        .map(|(index, text)| DiffLine {
            kind: DiffLineKind::Addition,
            old_lineno: None,
            new_lineno: Some(index as u32 + 1),
            text: text.to_string(),
        })
        .collect();

    FileDiff {
        hunks: vec![DiffHunk {
            old_start: 0,
            new_start: 1,
            lines,
        }],
        binary: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideCell {
    pub lineno: u32,
    pub text: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SideBySideRow {
    pub left: Option<SideBySideCell>,
    pub right: Option<SideBySideCell>,
}

/// Interleaves a hunk's lines into aligned side-by-side rows: context lines occupy both columns
/// on the same row; a contiguous deletion block and the addition block that follows it are
/// zipped row-by-row, with the shorter side left blank for any extra rows on the longer side.
pub fn hunk_to_side_by_side(hunk: &DiffHunk) -> Vec<SideBySideRow> {
    let mut rows = Vec::new();
    let mut i = 0;

    while i < hunk.lines.len() {
        let line = &hunk.lines[i];
        match line.kind {
            DiffLineKind::Context => {
                rows.push(SideBySideRow {
                    left: Some(SideBySideCell {
                        lineno: line.old_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: false,
                    }),
                    right: Some(SideBySideCell {
                        lineno: line.new_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: false,
                    }),
                });
                i += 1;
            }
            DiffLineKind::Deletion | DiffLineKind::Addition => {
                let deletions_start = i;
                while i < hunk.lines.len() && hunk.lines[i].kind == DiffLineKind::Deletion {
                    i += 1;
                }
                let deletions = &hunk.lines[deletions_start..i];

                let additions_start = i;
                while i < hunk.lines.len() && hunk.lines[i].kind == DiffLineKind::Addition {
                    i += 1;
                }
                let additions = &hunk.lines[additions_start..i];

                let paired = deletions.len().max(additions.len());
                for j in 0..paired {
                    let left = deletions.get(j).map(|line| SideBySideCell {
                        lineno: line.old_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: true,
                    });
                    let right = additions.get(j).map(|line| SideBySideCell {
                        lineno: line.new_lineno.unwrap_or(0),
                        text: line.text.clone(),
                        changed: true,
                    });
                    rows.push(SideBySideRow { left, right });
                }
            }
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_hunk_with_context_addition_and_deletion() {
        let diff = "diff --git a/f.rs b/f.rs\n\
index abc123..def456 100644\n\
--- a/f.rs\n\
+++ b/f.rs\n\
@@ -1,4 +1,4 @@\n\
 unchanged\n\
-old line\n\
+new line\n\
 tail\n";

        let file_diff = parse_unified_diff(diff);
        assert!(!file_diff.binary);
        assert_eq!(file_diff.hunks.len(), 1);
        let hunk = &file_diff.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.lines.len(), 4);
        assert_eq!(hunk.lines[0].kind, DiffLineKind::Context);
        assert_eq!(hunk.lines[1].kind, DiffLineKind::Deletion);
        assert_eq!(hunk.lines[1].text, "old line");
        assert_eq!(hunk.lines[2].kind, DiffLineKind::Addition);
        assert_eq!(hunk.lines[2].text, "new line");
        assert_eq!(hunk.lines[3].kind, DiffLineKind::Context);
    }

    #[test]
    fn parses_multiple_hunks() {
        let diff = "diff --git a/f.rs b/f.rs\n\
--- a/f.rs\n\
+++ b/f.rs\n\
@@ -1,2 +1,2 @@\n\
-a\n\
+b\n\
@@ -10,2 +10,2 @@\n\
-c\n\
+d\n";
        let file_diff = parse_unified_diff(diff);
        assert_eq!(file_diff.hunks.len(), 2);
        assert_eq!(file_diff.hunks[1].old_start, 10);
    }

    #[test]
    fn ignores_no_newline_marker() {
        let diff = "--- a/f.rs\n\
+++ b/f.rs\n\
@@ -1,1 +1,1 @@\n\
-old\n\
\\ No newline at end of file\n\
+new\n";
        let file_diff = parse_unified_diff(diff);
        assert_eq!(file_diff.hunks[0].lines.len(), 2);
    }

    #[test]
    fn detects_binary_file_diff() {
        let diff = "diff --git a/img.png b/img.png\n\
Binary files a/img.png and b/img.png differ\n";
        let file_diff = parse_unified_diff(diff);
        assert!(file_diff.binary);
        assert!(file_diff.hunks.is_empty());
    }

    #[test]
    fn empty_diff_yields_no_hunks() {
        let file_diff = parse_unified_diff("");
        assert!(!file_diff.binary);
        assert!(file_diff.hunks.is_empty());
    }

    #[test]
    fn side_by_side_pairs_equal_length_blocks() {
        let hunk = DiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_lineno: Some(1),
                    new_lineno: None,
                    text: "old1".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_lineno: None,
                    new_lineno: Some(1),
                    text: "new1".into(),
                },
            ],
        };
        let rows = hunk_to_side_by_side(&hunk);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].left.as_ref().unwrap().text, "old1");
        assert_eq!(rows[0].right.as_ref().unwrap().text, "new1");
    }

    #[test]
    fn side_by_side_pads_shorter_side_with_blank_cells() {
        let hunk = DiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_lineno: Some(1),
                    new_lineno: None,
                    text: "old1".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_lineno: Some(2),
                    new_lineno: None,
                    text: "old2".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_lineno: None,
                    new_lineno: Some(1),
                    text: "new1".into(),
                },
            ],
        };
        let rows = hunk_to_side_by_side(&hunk);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().unwrap().text, "old1");
        assert_eq!(rows[0].right.as_ref().unwrap().text, "new1");
        assert_eq!(rows[1].left.as_ref().unwrap().text, "old2");
        assert!(rows[1].right.is_none());
    }

    #[test]
    fn side_by_side_keeps_context_rows_unchanged() {
        let hunk = DiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(1),
                new_lineno: Some(1),
                text: "same".into(),
            }],
        };
        let rows = hunk_to_side_by_side(&hunk);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].left.as_ref().unwrap().changed);
        assert!(!rows[0].right.as_ref().unwrap().changed);
    }

    #[test]
    fn untracked_file_diff_marks_all_lines_as_additions() {
        use crate::workspace::git::test_support::temp_test_dir;

        let repo = temp_test_dir("untracked-diff");
        std::fs::write(repo.join("new.txt"), "one\ntwo\n").unwrap();

        let file_diff = git_untracked_file_diff(&repo, "new.txt");
        assert!(!file_diff.binary);
        assert_eq!(file_diff.hunks.len(), 1);
        assert_eq!(file_diff.hunks[0].lines.len(), 2);
        assert!(file_diff.hunks[0]
            .lines
            .iter()
            .all(|line| line.kind == DiffLineKind::Addition));

        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn oversized_untracked_file_is_not_loaded() {
        use crate::workspace::git::test_support::temp_test_dir;

        let repo = temp_test_dir("untracked-diff-oversized");
        // Just over the 2 MiB preview cap.
        let big = vec![b'a'; 2 * 1024 * 1024 + 1];
        std::fs::write(repo.join("huge.log"), &big).unwrap();

        let file_diff = git_untracked_file_diff(&repo, "huge.log");
        assert!(file_diff.binary, "oversized files must not be previewed");
        assert!(file_diff.hunks.is_empty(), "no content should be retained");

        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn untracked_file_diff_reports_non_utf8_as_binary() {
        use crate::workspace::git::test_support::temp_test_dir;

        let repo = temp_test_dir("untracked-diff-binary");
        std::fs::write(repo.join("blob.bin"), [0xff, 0xfe, 0x00, 0xff]).unwrap();

        let file_diff = git_untracked_file_diff(&repo, "blob.bin");
        assert!(file_diff.binary);
        assert!(file_diff.hunks.is_empty());

        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn git_file_diff_distinguishes_staged_from_unstaged() {
        use crate::workspace::git::test_support::{run_git, temp_test_dir};

        let repo = temp_test_dir("file-diff-staged-vs-unstaged");
        run_git(&repo, &["init", "--quiet"]);
        run_git(&repo, &["config", "user.email", "herdr@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Herdr Test"]);
        std::fs::write(repo.join("f.txt"), "line1\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "--quiet", "-m", "initial"]);

        std::fs::write(repo.join("f.txt"), "line1\nstaged-edit\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        std::fs::write(repo.join("f.txt"), "line1\nstaged-edit\nunstaged-edit\n").unwrap();

        let staged_diff = git_file_diff(&repo, "f.txt", true).expect("staged diff");
        let unstaged_diff = git_file_diff(&repo, "f.txt", false).expect("unstaged diff");

        assert!(staged_diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.kind == DiffLineKind::Addition && line.text == "staged-edit"));
        assert!(unstaged_diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.kind == DiffLineKind::Addition && line.text == "unstaged-edit"));

        let _ = std::fs::remove_dir_all(repo);
    }
}
