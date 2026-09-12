use super::*;
use crossterm::event::{KeyCode, KeyModifiers};

impl ClientShellState {
    pub(super) fn toggle_sidebar_git_view(&mut self, outcome: &mut ClientShellInput) {
        let next = match self.sidebar_view {
            SidebarSpacesView::Spaces => SidebarSpacesView::Git,
            SidebarSpacesView::Git => SidebarSpacesView::Spaces,
        };
        self.set_sidebar_view(next, outcome);
    }

    pub(super) fn set_sidebar_view(
        &mut self,
        view: SidebarSpacesView,
        outcome: &mut ClientShellInput,
    ) {
        if self.sidebar_view == view {
            return;
        }
        self.sidebar_view = view;
        self.git_panel = ClientGitPanelState::default();
        outcome.repaint = true;
        self.sync_git_panel_watch(outcome);
    }

    /// Tells the server whether the focused workspace's git panel is currently visible in this
    /// client, so the background working-tree refresh only runs while someone can see it.
    fn sync_git_panel_watch(&mut self, outcome: &mut ClientShellInput) {
        let Some(workspace_id) = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_workspace_id.clone())
        else {
            return;
        };
        let active = self.sidebar_view == SidebarSpacesView::Git;
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitPanelSetActive(
                crate::api::schema::GitPanelSetActiveParams {
                    workspace_id,
                    active,
                },
            ),
            PendingEndpointKind::Generic,
            outcome,
        );
    }

    fn focused_git_workspace_id(&self) -> Option<String> {
        self.snapshot
            .as_deref()?
            .focused_workspace_id
            .as_ref()
            .cloned()
    }

    fn focused_git_working_tree(&self) -> Option<&crate::protocol::ClientShellGitWorkingTree> {
        let snapshot = self.snapshot.as_deref()?;
        let workspace_id = snapshot.focused_workspace_id.as_deref()?;
        snapshot
            .workspaces
            .iter()
            .find(|ws| ws.workspace_id == workspace_id)?
            .git_working_tree
            .as_ref()
    }

    fn git_panel_row_count(&self) -> usize {
        self.focused_git_working_tree()
            .map(|working_tree| working_tree.staged.len() + working_tree.unstaged.len())
            .unwrap_or(0)
    }

    fn selected_git_file_path(&self) -> Option<String> {
        let working_tree = self.focused_git_working_tree()?;
        working_tree
            .staged
            .iter()
            .chain(&working_tree.unstaged)
            .nth(self.git_panel.selected)
            .map(|entry| entry.path.clone())
    }

    fn move_git_panel_selection(&mut self, delta: i32, outcome: &mut ClientShellInput) {
        let count = self.git_panel_row_count();
        if count == 0 {
            return;
        }
        let next = (self.git_panel.selected as i32 + delta).clamp(0, count as i32 - 1) as usize;
        if next != self.git_panel.selected {
            self.git_panel.selected = next;
            outcome.repaint = true;
        }
    }

    fn send_git_file_action(
        &mut self,
        build: impl FnOnce(crate::api::schema::GitFileTargetParams) -> crate::api::schema::Method,
        outcome: &mut ClientShellInput,
    ) {
        let Some(workspace_id) = self.focused_git_workspace_id() else {
            return;
        };
        let Some(path) = self.selected_git_file_path() else {
            return;
        };
        self.push_endpoint_method_with_kind(
            build(crate::api::schema::GitFileTargetParams { workspace_id, path }),
            PendingEndpointKind::GitFileAction,
            outcome,
        );
    }

    fn stage_selected_git_file(&mut self, outcome: &mut ClientShellInput) {
        self.send_git_file_action(crate::api::schema::Method::GitFileStage, outcome);
    }

    fn unstage_selected_git_file(&mut self, outcome: &mut ClientShellInput) {
        self.send_git_file_action(crate::api::schema::Method::GitFileUnstage, outcome);
    }

    fn confirm_discard(&mut self, path: String, outcome: &mut ClientShellInput) {
        let Some(workspace_id) = self.focused_git_workspace_id() else {
            return;
        };
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitFileDiscard(crate::api::schema::GitFileTargetParams {
                workspace_id,
                path,
            }),
            PendingEndpointKind::GitFileAction,
            outcome,
        );
    }

    fn submit_git_commit(&mut self, outcome: &mut ClientShellInput) {
        let message = self.git_panel.commit_message.trim().to_string();
        if message.is_empty() {
            return;
        }
        let Some(workspace_id) = self.focused_git_workspace_id() else {
            return;
        };
        self.git_panel.commit_in_flight = true;
        outcome.repaint = true;
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitCommit(crate::api::schema::GitCommitParams {
                workspace_id,
                message,
            }),
            PendingEndpointKind::GitCommit,
            outcome,
        );
    }

    /// Routes a key to the Git panel while it is open. Returns whether the key was consumed —
    /// callers must not forward a consumed key to the focused pane or ordinary keybind
    /// resolution.
    pub(super) fn route_git_panel_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));

        if let Some(path) = self.git_panel.pending_discard.clone() {
            if modifiers.is_empty() && matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                self.git_panel.pending_discard = None;
                self.confirm_discard(path, outcome);
            } else {
                self.git_panel.pending_discard = None;
                outcome.repaint = true;
            }
            return true;
        }

        match self.git_panel.focus {
            GitSidebarFocus::FileList => match code {
                KeyCode::Tab if modifiers.is_empty() => {
                    self.git_panel.focus = GitSidebarFocus::CommitBox;
                    outcome.repaint = true;
                    true
                }
                KeyCode::Up | KeyCode::Char('k') if modifiers.is_empty() => {
                    self.move_git_panel_selection(-1, outcome);
                    true
                }
                KeyCode::Down | KeyCode::Char('j') if modifiers.is_empty() => {
                    self.move_git_panel_selection(1, outcome);
                    true
                }
                KeyCode::Char('s') if modifiers.is_empty() => {
                    self.stage_selected_git_file(outcome);
                    true
                }
                KeyCode::Char('u') if modifiers.is_empty() => {
                    self.unstage_selected_git_file(outcome);
                    true
                }
                KeyCode::Char('d') if modifiers.is_empty() => {
                    if let Some(path) = self.selected_git_file_path() {
                        self.git_panel.pending_discard = Some(path);
                        outcome.repaint = true;
                    }
                    true
                }
                KeyCode::Esc if modifiers.is_empty() => {
                    self.set_sidebar_view(SidebarSpacesView::Spaces, outcome);
                    true
                }
                _ => false,
            },
            GitSidebarFocus::CommitBox => match code {
                KeyCode::Tab if modifiers.is_empty() => {
                    self.git_panel.focus = GitSidebarFocus::FileList;
                    outcome.repaint = true;
                    true
                }
                KeyCode::Enter if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_git_commit(outcome);
                    true
                }
                KeyCode::Enter if modifiers.is_empty() => {
                    self.git_panel.commit_message.push('\n');
                    outcome.repaint = true;
                    true
                }
                KeyCode::Backspace if modifiers.is_empty() => {
                    self.git_panel.commit_message.pop();
                    outcome.repaint = true;
                    true
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.git_panel.commit_message.push(c);
                    outcome.repaint = true;
                    true
                }
                KeyCode::Esc if modifiers.is_empty() => {
                    self.git_panel.focus = GitSidebarFocus::FileList;
                    outcome.repaint = true;
                    true
                }
                _ => false,
            },
        }
    }

    /// Handles the response to a `git.file.*`/`git.commit` request. Returns whether the client
    /// needs to repaint.
    pub(super) fn handle_git_endpoint_result(
        &mut self,
        kind: PendingEndpointKind,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) -> bool {
        match kind {
            PendingEndpointKind::GitFileAction => {
                self.git_panel.last_error = result.err().map(|error| error.message);
                true
            }
            PendingEndpointKind::GitCommit => {
                self.git_panel.commit_in_flight = false;
                match result {
                    Ok(_) => {
                        self.git_panel.commit_message.clear();
                        self.git_panel.last_error = None;
                    }
                    Err(error) => {
                        self.git_panel.last_error = Some(error.message);
                    }
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::protocol::{
        ClientShellGitFileEntry, ClientShellGitFileStatus, ClientShellGitWorkingTree,
    };

    fn test_state_with_working_tree(working_tree: ClientShellGitWorkingTree) -> ClientShellState {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        let mut snapshot = super::super::tests::snapshot();
        snapshot.workspaces[0].git_repo = true;
        snapshot.workspaces[0].git_working_tree = Some(working_tree);
        snapshot.focused_workspace_id = Some(snapshot.workspaces[0].workspace_id.clone());
        let snapshot = Box::new(snapshot);
        state.endpoints[0].status = ClientEndpointStatus::Online;
        state.endpoints[0].snapshot = Some(snapshot.clone());
        state.snapshot = Some(snapshot);
        state.sidebar_view = SidebarSpacesView::Git;
        state
    }

    fn one_unstaged_file() -> ClientShellGitWorkingTree {
        ClientShellGitWorkingTree {
            staged: Vec::new(),
            unstaged: vec![ClientShellGitFileEntry {
                path: "f.rs".into(),
                original_path: None,
                status: ClientShellGitFileStatus::Modified,
            }],
        }
    }

    fn key(code: KeyCode) -> crate::input::TerminalKey {
        crate::input::TerminalKey::new(code, KeyModifiers::empty())
    }

    fn ctrl_key(code: KeyCode) -> crate::input::TerminalKey {
        crate::input::TerminalKey::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn stage_key_sends_stage_request_for_selected_file() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&key(KeyCode::Char('s')), &mut outcome);

        assert!(consumed);
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitFileStage(params) if params.path == "f.rs"
        ));
    }

    #[test]
    fn discard_key_requires_confirmation() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let mut outcome = ClientShellInput::default();

        state.route_git_panel_key(&key(KeyCode::Char('d')), &mut outcome);
        assert_eq!(state.git_panel.pending_discard.as_deref(), Some("f.rs"));
        assert!(outcome.actions.is_empty());

        let mut outcome = ClientShellInput::default();
        let consumed = state.route_git_panel_key(&key(KeyCode::Char('y')), &mut outcome);
        assert!(consumed);
        assert!(state.git_panel.pending_discard.is_none());
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitFileDiscard(params) if params.path == "f.rs"
        ));
    }

    #[test]
    fn discard_key_cancels_on_any_other_key() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let mut outcome = ClientShellInput::default();
        state.route_git_panel_key(&key(KeyCode::Char('d')), &mut outcome);

        let mut outcome = ClientShellInput::default();
        let consumed = state.route_git_panel_key(&key(KeyCode::Char('n')), &mut outcome);

        assert!(consumed);
        assert!(state.git_panel.pending_discard.is_none());
        assert!(outcome.actions.is_empty());
    }

    #[test]
    fn tab_switches_focus_between_file_list_and_commit_box() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let mut outcome = ClientShellInput::default();

        state.route_git_panel_key(&key(KeyCode::Tab), &mut outcome);
        assert_eq!(state.git_panel.focus, GitSidebarFocus::CommitBox);

        state.route_git_panel_key(&key(KeyCode::Tab), &mut outcome);
        assert_eq!(state.git_panel.focus, GitSidebarFocus::FileList);
    }

    #[test]
    fn typing_in_commit_box_appends_to_the_message() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        let mut outcome = ClientShellInput::default();

        state.route_git_panel_key(&key(KeyCode::Char('h')), &mut outcome);
        state.route_git_panel_key(&key(KeyCode::Char('i')), &mut outcome);

        assert_eq!(state.git_panel.commit_message, "hi");
    }

    #[test]
    fn ctrl_enter_submits_a_non_empty_commit_message() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        state.git_panel.commit_message = "fix bug".into();
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&ctrl_key(KeyCode::Enter), &mut outcome);

        assert!(consumed);
        assert!(state.git_panel.commit_in_flight);
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitCommit(params) if params.message == "fix bug"
        ));
    }

    #[test]
    fn ctrl_enter_does_nothing_for_an_empty_message() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        let mut outcome = ClientShellInput::default();

        state.route_git_panel_key(&ctrl_key(KeyCode::Enter), &mut outcome);

        assert!(!state.git_panel.commit_in_flight);
        assert!(outcome.actions.is_empty());
    }

    #[test]
    fn git_commit_success_clears_the_draft_message() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.commit_in_flight = true;
        state.git_panel.commit_message = "fix bug".into();

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitCommit,
            Ok(crate::api::schema::ResponseResult::Ok {}),
        );

        assert!(repaint);
        assert!(!state.git_panel.commit_in_flight);
        assert!(state.git_panel.commit_message.is_empty());
        assert!(state.git_panel.last_error.is_none());
    }

    #[test]
    fn git_commit_failure_keeps_the_draft_and_records_the_error() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.commit_in_flight = true;
        state.git_panel.commit_message = "fix bug".into();

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitCommit,
            Err(ClientShellEndpointError {
                code: Some("git_mutation_failed".into()),
                message: "nothing to commit".into(),
            }),
        );

        assert!(repaint);
        assert!(!state.git_panel.commit_in_flight);
        assert_eq!(state.git_panel.commit_message, "fix bug");
        assert_eq!(
            state.git_panel.last_error.as_deref(),
            Some("nothing to commit")
        );
    }
}
