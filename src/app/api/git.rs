mod deferred;

use std::path::PathBuf;

use crate::api::schema;
use crate::app::popup::PopupGeometry;
use crate::app::App;

use super::responses::{encode_error, encode_success};

pub(super) struct GitApiFailure {
    code: &'static str,
    message: String,
}

impl GitApiFailure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl App {
    /// Resolves a `workspace_id` from a `git.*` request into its index and repo root. Every
    /// `git.*` handler needs exactly this, and only this: none of them touch pane/tab layout.
    pub(super) fn resolve_git_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<(usize, PathBuf), GitApiFailure> {
        let Some(ws_idx) = self.parse_workspace_id(workspace_id) else {
            return Err(GitApiFailure::new(
                "workspace_not_found",
                format!("workspace {workspace_id} not found"),
            ));
        };
        let Some(repo_root) = self.state.workspaces[ws_idx]
            .git_space()
            .map(|space| space.repo_root.clone())
        else {
            return Err(GitApiFailure::new(
                "not_a_git_repository",
                "workspace is not inside a git repository",
            ));
        };
        Ok((ws_idx, repo_root))
    }

    /// Runs a fixed argv git command in a popup pane rooted at `workspace_id`'s repo. Argv is
    /// always built by the caller from server-known strings (a fixed subcommand, or a branch/
    /// stash name that came out of `git branch`/`git stash list`) — never from an unvalidated
    /// client string interpolated into a shell command line.
    fn spawn_git_repo_popup(
        &mut self,
        id: String,
        workspace_id: String,
        argv: Vec<String>,
    ) -> String {
        let (ws_idx, repo_root) = match self.resolve_git_workspace(&workspace_id) {
            Ok(resolved) => resolved,
            Err(err) => return encode_error(id, err.code, err.message),
        };
        match self.spawn_popup_argv_command_in_workspace(
            ws_idx,
            &argv,
            Some(repo_root),
            Vec::new(),
            PopupGeometry::default(),
        ) {
            Ok(()) => encode_success(id, schema::ResponseResult::Ok {}),
            Err(err) => encode_error(id, "popup_unavailable", err.to_string()),
        }
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    pub(crate) fn handle_git_repo_fetch(
        &mut self,
        id: String,
        params: schema::GitRepoCommandParams,
    ) -> String {
        self.spawn_git_repo_popup(
            id,
            params.workspace_id,
            Self::argv(&["git", "fetch", "--all", "--prune"]),
        )
    }

    pub(crate) fn handle_git_repo_pull(
        &mut self,
        id: String,
        params: schema::GitRepoCommandParams,
    ) -> String {
        self.spawn_git_repo_popup(id, params.workspace_id, Self::argv(&["git", "pull"]))
    }

    pub(crate) fn handle_git_repo_push(
        &mut self,
        id: String,
        params: schema::GitRepoCommandParams,
    ) -> String {
        self.spawn_git_repo_popup(id, params.workspace_id, Self::argv(&["git", "push"]))
    }

    pub(crate) fn handle_git_repo_log(
        &mut self,
        id: String,
        params: schema::GitRepoCommandParams,
    ) -> String {
        self.spawn_git_repo_popup(
            id,
            params.workspace_id,
            Self::argv(&[
                "git",
                "log",
                "--oneline",
                "--graph",
                "--decorate",
                "-n",
                "200",
            ]),
        )
    }

    pub(crate) fn handle_git_stash_push(
        &mut self,
        id: String,
        params: schema::GitRepoCommandParams,
    ) -> String {
        self.spawn_git_repo_popup(
            id,
            params.workspace_id,
            Self::argv(&["git", "stash", "push"]),
        )
    }

    /// `stash pop` rather than `apply`: leaving an applied stash on the stack is a common source
    /// of duplicate work later, so the picker action drops it once applied.
    pub(crate) fn handle_git_stash_pop(
        &mut self,
        id: String,
        params: schema::GitStashPopParams,
    ) -> String {
        let argv = vec![
            "git".to_string(),
            "stash".to_string(),
            "pop".to_string(),
            params.stash_ref,
        ];
        self.spawn_git_repo_popup(id, params.workspace_id, argv)
    }

    pub(crate) fn handle_git_branch_create(
        &mut self,
        id: String,
        params: schema::GitBranchNameParams,
    ) -> String {
        // `switch -c` creates and checks out in one step, and reports in the popup if the name
        // is already taken or invalid.
        let argv = vec![
            "git".to_string(),
            "switch".to_string(),
            "-c".to_string(),
            params.name,
        ];
        self.spawn_git_repo_popup(id, params.workspace_id, argv)
    }

    pub(crate) fn handle_git_branch_switch(
        &mut self,
        id: String,
        params: schema::GitBranchNameParams,
    ) -> String {
        let argv = vec!["git".to_string(), "switch".to_string(), params.name];
        self.spawn_git_repo_popup(id, params.workspace_id, argv)
    }

    /// `-d` refuses to drop a branch that is not merged; the popup shows git's warning instead
    /// of silently discarding commits.
    pub(crate) fn handle_git_branch_delete(
        &mut self,
        id: String,
        params: schema::GitBranchNameParams,
    ) -> String {
        let argv = vec![
            "git".to_string(),
            "branch".to_string(),
            "-d".to_string(),
            params.name,
        ];
        self.spawn_git_repo_popup(id, params.workspace_id, argv)
    }
}
