mod actions;
mod config;
#[cfg(test)]
mod config_tests;
mod diff;
mod discovery;
mod lists;
mod status;
#[cfg(test)]
pub(super) mod test_support;
mod worktree_status;

pub(crate) use self::discovery::automatic_workspace_label;

pub use self::{
    actions::{run_git_commit, run_git_file_action, GitFileActionKind},
    diff::{git_file_diff, git_untracked_file_diff, DiffHunk, DiffLine, DiffLineKind, FileDiff},
    discovery::{
        derive_label_from_cwd, fallback_label_from_cwd, git_branch, git_space_metadata,
        GitSpaceMetadata,
    },
    lists::{git_branch_list, git_stash_list, GitListEntry},
    status::{
        git_status_cache_key, git_status_cache_key_for_space,
        git_status_snapshot_for_cwd_with_demand, GitStatusCacheEntry, GitStatusRefreshDemand,
    },
    worktree_status::{
        git_working_tree_status, GitFileEntry, GitFileStatusKind, GitWorkingTreeStatus,
    },
};
