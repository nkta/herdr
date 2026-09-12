use std::sync::mpsc::Sender;

use crate::api::schema::{self, Method};
use crate::app::App;
use crate::events::AppEvent;
use crate::workspace::GitFileActionKind;

use super::super::responses::{encode_error, encode_success};

impl App {
    /// Dispatches the `git.*` methods that shell a blocking `git` command from a background
    /// thread instead of answering inline. Read-only ones (`diff.get`, `picker.list`) reply
    /// straight from that thread; mutations reply through `AppEvent::GitMutationFinished` so the
    /// main loop can also force a working-tree refresh.
    pub(crate) fn handle_deferred_git_api_request(
        &mut self,
        request: schema::Request,
        respond_to: Sender<String>,
    ) -> bool {
        match request.method {
            Method::GitFileStage(params) => {
                self.start_git_file_mutation(
                    request.id,
                    params.workspace_id,
                    params.path,
                    GitFileActionKind::Stage,
                    respond_to,
                );
                true
            }
            Method::GitFileUnstage(params) => {
                self.start_git_file_mutation(
                    request.id,
                    params.workspace_id,
                    params.path,
                    GitFileActionKind::Unstage,
                    respond_to,
                );
                true
            }
            Method::GitFileDiscard(params) => {
                self.start_git_file_mutation(
                    request.id,
                    params.workspace_id,
                    params.path,
                    GitFileActionKind::Discard,
                    respond_to,
                );
                true
            }
            Method::GitCommit(params) => {
                self.start_git_commit_request(
                    request.id,
                    params.workspace_id,
                    params.message,
                    respond_to,
                );
                true
            }
            Method::GitCommitMessageGenerate(params) => {
                self.start_git_commit_message_generate(request.id, params.workspace_id, respond_to);
                true
            }
            Method::GitDiffGet(params) => {
                self.start_git_diff_get(request.id, params, respond_to);
                true
            }
            Method::GitPickerList(params) => {
                self.start_git_picker_list(request.id, params, respond_to);
                true
            }
            _ => false,
        }
    }

    fn start_git_file_mutation(
        &mut self,
        id: String,
        workspace_id: String,
        path: String,
        action: GitFileActionKind,
        respond_to: Sender<String>,
    ) {
        let (_, repo_root) = match self.resolve_git_workspace(&workspace_id) {
            Ok(resolved) => resolved,
            Err(err) => {
                let _ = respond_to.send(encode_error(id, err.code, err.message));
                return;
            }
        };
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = crate::workspace::run_git_file_action(&repo_root, &path, action);
            let _ = event_tx.blocking_send(AppEvent::GitMutationFinished {
                request_id: id,
                workspace_id,
                respond_to,
                result,
            });
        });
    }

    fn start_git_commit_request(
        &mut self,
        id: String,
        workspace_id: String,
        message: String,
        respond_to: Sender<String>,
    ) {
        let (_, repo_root) = match self.resolve_git_workspace(&workspace_id) {
            Ok(resolved) => resolved,
            Err(err) => {
                let _ = respond_to.send(encode_error(id, err.code, err.message));
                return;
            }
        };
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = crate::workspace::run_git_commit(&repo_root, &message);
            let _ = event_tx.blocking_send(AppEvent::GitMutationFinished {
                request_id: id,
                workspace_id,
                respond_to,
                result,
            });
        });
    }

    const COMMIT_MESSAGE_GENERATION_TIMEOUT: std::time::Duration =
        std::time::Duration::from_secs(45);

    fn start_git_commit_message_generate(
        &mut self,
        id: String,
        workspace_id: String,
        respond_to: Sender<String>,
    ) {
        let (_, repo_root) = match self.resolve_git_workspace(&workspace_id) {
            Ok(resolved) => resolved,
            Err(err) => {
                let _ = respond_to.send(encode_error(id, err.code, err.message));
                return;
            }
        };
        let Some(agent_id) = self.state.active_commit_agent.clone() else {
            let _ = respond_to.send(encode_error(
                id,
                "no_active_commit_agent",
                "no commit agent is configured as active",
            ));
            return;
        };
        let Some(agent) = self
            .state
            .commit_agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .cloned()
        else {
            let _ = respond_to.send(encode_error(
                id,
                "unknown_commit_agent",
                format!("active commit agent \"{agent_id}\" is not configured"),
            ));
            return;
        };
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = crate::workspace::git_cached_diff_text(&repo_root)
                .filter(|diff| !diff.trim().is_empty())
                .ok_or_else(|| "no staged changes to describe".to_string())
                .and_then(|diff| {
                    let prompt = crate::workspace::build_commit_message_prompt(&diff);
                    crate::workspace::generate_commit_message(
                        &repo_root,
                        &agent.command,
                        &agent.args,
                        &prompt,
                        Self::COMMIT_MESSAGE_GENERATION_TIMEOUT,
                    )
                });
            let _ = event_tx.blocking_send(AppEvent::GitCommitMessageGenerated {
                request_id: id,
                respond_to,
                result,
            });
        });
    }

    fn start_git_diff_get(
        &mut self,
        id: String,
        params: schema::GitDiffGetParams,
        respond_to: Sender<String>,
    ) {
        let (_, repo_root) = match self.resolve_git_workspace(&params.workspace_id) {
            Ok(resolved) => resolved,
            Err(err) => {
                let _ = respond_to.send(encode_error(id, err.code, err.message));
                return;
            }
        };
        std::thread::spawn(move || {
            let diff = if params.untracked {
                Some(crate::workspace::git_untracked_file_diff(
                    &repo_root,
                    &params.path,
                ))
            } else {
                crate::workspace::git_file_diff(&repo_root, &params.path, params.staged)
            };
            let response = match diff {
                Some(diff) => encode_success(
                    id,
                    schema::ResponseResult::GitFileDiff { diff: diff.into() },
                ),
                None => encode_error(id, "git_diff_failed", "failed to compute diff"),
            };
            let _ = respond_to.send(response);
        });
    }

    fn start_git_picker_list(
        &mut self,
        id: String,
        params: schema::GitPickerListParams,
        respond_to: Sender<String>,
    ) {
        let (_, repo_root) = match self.resolve_git_workspace(&params.workspace_id) {
            Ok(resolved) => resolved,
            Err(err) => {
                let _ = respond_to.send(encode_error(id, err.code, err.message));
                return;
            }
        };
        std::thread::spawn(move || {
            let entries = match params.kind {
                schema::GitPickerKind::Stash => crate::workspace::git_stash_list(&repo_root),
                schema::GitPickerKind::Branch => crate::workspace::git_branch_list(&repo_root),
            };
            let response = match entries {
                Some(entries) => encode_success(
                    id,
                    schema::ResponseResult::GitPickerList {
                        entries: entries.into_iter().map(Into::into).collect(),
                    },
                ),
                None => encode_error(id, "git_picker_list_failed", "failed to list entries"),
            };
            let _ = respond_to.send(response);
        });
    }
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
    fn stage_reports_workspace_not_found() {
        let mut app = test_app();
        let (tx, rx) = std::sync::mpsc::channel();

        let handled = app.handle_deferred_git_api_request(
            schema::Request {
                id: "req1".into(),
                method: Method::GitFileStage(schema::GitFileTargetParams {
                    workspace_id: "missing".into(),
                    path: "f.txt".into(),
                }),
            },
            tx,
        );

        assert!(handled);
        let response = rx.recv().expect("synchronous error response");
        assert!(response.contains("workspace_not_found"));
    }

    #[test]
    fn stage_reports_not_a_git_repository() {
        let mut app = test_app();
        app.state.workspaces.push(Workspace::test_new("plain"));
        let workspace_id = app.state.workspaces[0].id.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        app.handle_deferred_git_api_request(
            schema::Request {
                id: "req1".into(),
                method: Method::GitFileStage(schema::GitFileTargetParams {
                    workspace_id,
                    path: "f.txt".into(),
                }),
            },
            tx,
        );

        let response = rx.recv().expect("synchronous error response");
        assert!(response.contains("not_a_git_repository"));
    }

    #[test]
    fn generate_commit_message_reports_workspace_not_found() {
        let mut app = test_app();
        let (tx, rx) = std::sync::mpsc::channel();

        let handled = app.handle_deferred_git_api_request(
            schema::Request {
                id: "req1".into(),
                method: Method::GitCommitMessageGenerate(schema::GitCommitMessageGenerateParams {
                    workspace_id: "missing".into(),
                }),
            },
            tx,
        );

        assert!(handled);
        let response = rx.recv().expect("synchronous error response");
        assert!(response.contains("workspace_not_found"));
    }

    #[test]
    fn generate_commit_message_reports_no_active_agent() {
        let mut app = test_app();
        let mut workspace = Workspace::test_new("repo");
        workspace.cached_git_space = Some(crate::workspace::GitSpaceMetadata {
            key: "repo".into(),
            checkout_key: "repo".into(),
            repo_name: "repo".into(),
            repo_root: std::path::PathBuf::from("/tmp/does-not-matter"),
            is_linked_worktree: false,
        });
        app.state.workspaces.push(workspace);
        let workspace_id = app.state.workspaces[0].id.clone();
        app.state.active_commit_agent = None;
        let (tx, rx) = std::sync::mpsc::channel();

        let handled = app.handle_deferred_git_api_request(
            schema::Request {
                id: "req1".into(),
                method: Method::GitCommitMessageGenerate(schema::GitCommitMessageGenerateParams {
                    workspace_id,
                }),
            },
            tx,
        );

        assert!(handled);
        let response = rx.recv().expect("synchronous error response");
        assert!(response.contains("no_active_commit_agent"));
    }

    #[test]
    fn unrelated_method_is_not_handled() {
        let mut app = test_app();
        let (tx, _rx) = std::sync::mpsc::channel();

        let handled = app.handle_deferred_git_api_request(
            schema::Request {
                id: "req1".into(),
                method: Method::Ping(schema::PingParams::default()),
            },
            tx,
        );

        assert!(!handled);
    }
}
