use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitPanelSetActiveParams {
    pub workspace_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitFileTargetParams {
    pub workspace_id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitCommitParams {
    pub workspace_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitDiffGetParams {
    pub workspace_id: String,
    pub path: String,
    #[serde(default)]
    pub staged: bool,
    #[serde(default)]
    pub untracked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitPickerKind {
    Stash,
    Branch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitPickerListParams {
    pub workspace_id: String,
    pub kind: GitPickerKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitRepoCommandParams {
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitStashApplyParams {
    pub workspace_id: String,
    pub stash_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitBranchNameParams {
    pub workspace_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitDiffLine {
    pub kind: GitDiffLineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_lineno: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_lineno: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitDiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<GitDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct GitFileDiff {
    #[serde(default)]
    pub hunks: Vec<GitDiffHunk>,
    #[serde(default)]
    pub binary: bool,
}

/// One selectable stash or branch entry for `git.picker.list`. Named distinctly from
/// `crate::workspace::GitListEntry` (the internal parsing type this is converted from) to keep
/// the wire-facing API schema and the internal git-parsing layer decoupled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitPickerEntry {
    pub value: String,
    pub label: String,
}

impl From<crate::workspace::DiffLineKind> for GitDiffLineKind {
    fn from(kind: crate::workspace::DiffLineKind) -> Self {
        match kind {
            crate::workspace::DiffLineKind::Context => Self::Context,
            crate::workspace::DiffLineKind::Addition => Self::Addition,
            crate::workspace::DiffLineKind::Deletion => Self::Deletion,
        }
    }
}

impl From<crate::workspace::DiffLine> for GitDiffLine {
    fn from(line: crate::workspace::DiffLine) -> Self {
        Self {
            kind: line.kind.into(),
            old_lineno: line.old_lineno,
            new_lineno: line.new_lineno,
            text: line.text,
        }
    }
}

impl From<crate::workspace::DiffHunk> for GitDiffHunk {
    fn from(hunk: crate::workspace::DiffHunk) -> Self {
        Self {
            old_start: hunk.old_start,
            new_start: hunk.new_start,
            lines: hunk.lines.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::workspace::FileDiff> for GitFileDiff {
    fn from(diff: crate::workspace::FileDiff) -> Self {
        Self {
            hunks: diff.hunks.into_iter().map(Into::into).collect(),
            binary: diff.binary,
        }
    }
}

impl From<crate::workspace::GitListEntry> for GitPickerEntry {
    fn from(entry: crate::workspace::GitListEntry) -> Self {
        Self {
            value: entry.value,
            label: entry.label,
        }
    }
}
