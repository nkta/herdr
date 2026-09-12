use super::*;
use crossterm::event::{KeyCode, KeyModifiers};

impl ClientShellState {
    /// Bound to `prefix+t`. Shows the Git tab and gives it keyboard focus in one step; a second
    /// press while it's already focused toggles back to Spaces. Without this, leaving focus with
    /// `Esc` (which keeps the Git tab visible but unfocused) stranded the panel: pressing
    /// `prefix+t` again just flipped straight to Spaces, since it always toggled between the two
    /// views and never re-focused an already-visible one.
    pub(super) fn toggle_sidebar_git_view(&mut self, outcome: &mut ClientShellInput) {
        let git_view_focused =
            self.sidebar_view == SidebarSpacesView::Git && self.mode == ClientShellMode::SidebarGit;
        if git_view_focused {
            self.set_sidebar_view(SidebarSpacesView::Spaces, outcome);
            return;
        }
        self.set_sidebar_view(SidebarSpacesView::Git, outcome);
        if !self.sidebar_collapsed {
            self.mode = ClientShellMode::SidebarGit;
            outcome.repaint = true;
        }
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
        self.release_sidebar_git_focus_if_hidden(outcome);
    }

    /// Hands the keyboard back to the pane when the Git panel stops being visible (tab switched
    /// away, or the sidebar collapsed). `ClientShellMode::SidebarGit` routes every keystroke into
    /// the panel; leaving it active once the panel is hidden would either look frozen (keys land
    /// in an invisible commit box) or let `s`/`u`/`d` act on a file the user can no longer see.
    pub(super) fn release_sidebar_git_focus_if_hidden(&mut self, outcome: &mut ClientShellInput) {
        let panel_visible = self.sidebar_view == SidebarSpacesView::Git && !self.sidebar_collapsed;
        if panel_visible || self.mode != ClientShellMode::SidebarGit {
            return;
        }
        self.mode = ClientShellMode::Terminal;
        outcome.repaint = true;
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

    /// The selected file's path, whether it should be diffed against the index (staged), and
    /// whether it's untracked (diffed by reading it directly rather than via `git diff`).
    fn selected_git_file_target(&self) -> Option<(String, bool, bool)> {
        let working_tree = self.focused_git_working_tree()?;
        let staged_len = working_tree.staged.len();
        if self.git_panel.selected < staged_len {
            let entry = &working_tree.staged[self.git_panel.selected];
            Some((entry.path.clone(), true, false))
        } else {
            let entry = working_tree
                .unstaged
                .get(self.git_panel.selected - staged_len)?;
            let untracked = entry.status == crate::protocol::ClientShellGitFileStatus::Untracked;
            Some((entry.path.clone(), false, untracked))
        }
    }

    fn open_git_diff_overlay(&mut self, outcome: &mut ClientShellInput) {
        let Some(workspace_id) = self.focused_git_workspace_id() else {
            return;
        };
        let Some((path, staged, untracked)) = self.selected_git_file_target() else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::GitDiff(ClientGitDiffOverlay {
            path: path.clone(),
            staged,
            diff: None,
            scroll: 0,
            loading: true,
            error: None,
        }));
        outcome.repaint = true;
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitDiffGet(crate::api::schema::GitDiffGetParams {
                workspace_id,
                path: path.clone(),
                staged,
                untracked,
            }),
            PendingEndpointKind::GitDiffGet { path, staged },
            outcome,
        );
    }

    /// Routes a key while the diff overlay is open. Returns whether the key was consumed.
    pub(super) fn route_git_diff_overlay_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(ClientShellOverlay::GitDiff(overlay)) = self.overlay.as_mut() else {
            return false;
        };
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        match code {
            KeyCode::Esc if modifiers.is_empty() => {
                self.overlay = None;
                outcome.repaint = true;
                true
            }
            KeyCode::Up | KeyCode::Char('k') if modifiers.is_empty() => {
                overlay.scroll = overlay.scroll.saturating_sub(1);
                outcome.repaint = true;
                true
            }
            KeyCode::Down | KeyCode::Char('j') if modifiers.is_empty() => {
                overlay.scroll = overlay.scroll.saturating_add(1);
                outcome.repaint = true;
                true
            }
            KeyCode::PageUp if modifiers.is_empty() => {
                overlay.scroll = overlay.scroll.saturating_sub(20);
                outcome.repaint = true;
                true
            }
            KeyCode::PageDown if modifiers.is_empty() => {
                overlay.scroll = overlay.scroll.saturating_add(20);
                outcome.repaint = true;
                true
            }
            _ => true,
        }
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

    /// Generates a commit message with the active commit agent, replacing any draft already in
    /// the commit box. Runs server-side (`git.commit_message.generate`): the server owns the
    /// worktree and the configured agent binary.
    fn generate_commit_message(&mut self, outcome: &mut ClientShellInput) {
        if self.git_panel.generating_commit_message || self.git_panel.commit_in_flight {
            return;
        }
        let Some(workspace_id) = self.focused_git_workspace_id() else {
            return;
        };
        self.git_panel.generating_commit_message = true;
        outcome.repaint = true;
        if !self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitCommitMessageGenerate(
                crate::api::schema::GitCommitMessageGenerateParams { workspace_id },
            ),
            PendingEndpointKind::GitCommitMessageGenerate,
            outcome,
        ) {
            self.git_panel.generating_commit_message = false;
        }
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
                KeyCode::Enter if modifiers.is_empty() => {
                    self.open_git_diff_overlay(outcome);
                    true
                }
                KeyCode::Char('m') if modifiers.is_empty() => {
                    self.open_git_repo_menu(outcome);
                    true
                }
                KeyCode::Esc if modifiers.is_empty() => {
                    self.mode = ClientShellMode::Terminal;
                    outcome.repaint = true;
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
                KeyCode::Char('g') if modifiers == KeyModifiers::CONTROL => {
                    self.generate_commit_message(outcome);
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
                    self.mode = ClientShellMode::Terminal;
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
            PendingEndpointKind::GitCommitMessageGenerate => {
                self.git_panel.generating_commit_message = false;
                match result {
                    Ok(crate::api::schema::ResponseResult::GitCommitMessageGenerated {
                        message,
                    }) => {
                        self.git_panel.commit_message = message;
                        self.git_panel.last_error = None;
                    }
                    Ok(_) => {
                        self.git_panel.last_error = Some(
                            "endpoint returned an unexpected commit message generation result"
                                .into(),
                        );
                    }
                    Err(error) => self.git_panel.last_error = Some(error.message),
                }
                true
            }
            PendingEndpointKind::GitDiffGet { path, staged } => {
                let Some(ClientShellOverlay::GitDiff(overlay)) = self.overlay.as_mut() else {
                    return false;
                };
                // A stale response for a diff the user has since navigated away from.
                if overlay.path != path || overlay.staged != staged {
                    return false;
                }
                overlay.loading = false;
                match result {
                    Ok(crate::api::schema::ResponseResult::GitFileDiff { diff }) => {
                        overlay.diff = Some(diff);
                        overlay.error = None;
                    }
                    Ok(_) => {
                        overlay.error = Some("endpoint returned an unexpected diff result".into());
                    }
                    Err(error) => {
                        overlay.error = Some(error.message);
                    }
                }
                true
            }
            PendingEndpointKind::GitPickerList => {
                let Some(ClientShellOverlay::GitPicker(picker)) = self.overlay.as_mut() else {
                    return false;
                };
                picker.loading = false;
                match result {
                    Ok(crate::api::schema::ResponseResult::GitPickerList { entries }) => {
                        picker.entries = entries;
                        picker.selected = 0;
                        picker.error = None;
                    }
                    Ok(_) => {
                        picker.error = Some("endpoint returned an unexpected picker result".into());
                    }
                    Err(error) => {
                        picker.error = Some(error.message);
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Opens the repository-wide command menu (fetch/pull/push/log/stash/branches) for the
    /// focused workspace. Position is approximate — `render_context_menu` clamps it to the
    /// screen, so this doesn't need the git panel's exact on-screen rect.
    pub(super) fn open_git_repo_menu(&mut self, outcome: &mut ClientShellInput) {
        let Some(workspace_id) = self.focused_git_workspace_id() else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::GitRepo { workspace_id },
            x: 2,
            y: 3,
            highlighted: 0,
        }));
        outcome.repaint = true;
    }

    pub(super) fn activate_git_repo_context_action(
        &mut self,
        workspace_id: String,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        use crate::api::schema::{GitRepoCommandParams, Method};
        use ClientContextMenuAction as Action;

        match action {
            Action::GitFetch => {
                self.push_endpoint_method(
                    Method::GitRepoFetch(GitRepoCommandParams { workspace_id }),
                    outcome,
                );
            }
            Action::GitPull => {
                self.push_endpoint_method(
                    Method::GitRepoPull(GitRepoCommandParams { workspace_id }),
                    outcome,
                );
            }
            Action::GitPush => {
                self.push_endpoint_method(
                    Method::GitRepoPush(GitRepoCommandParams { workspace_id }),
                    outcome,
                );
            }
            Action::GitLog => {
                self.push_endpoint_method(
                    Method::GitRepoLog(GitRepoCommandParams { workspace_id }),
                    outcome,
                );
            }
            Action::GitStashPush => {
                self.push_endpoint_method(
                    Method::GitStashPush(GitRepoCommandParams { workspace_id }),
                    outcome,
                );
            }
            Action::GitStashApply => {
                self.open_git_picker(workspace_id, GitPickerPurpose::ApplyStash, outcome);
            }
            Action::GitNewBranch => {
                self.open_git_branch_create_overlay(workspace_id);
                outcome.repaint = true;
            }
            Action::GitSwitchBranch => {
                self.open_git_picker(workspace_id, GitPickerPurpose::SwitchBranch, outcome);
            }
            Action::GitDeleteBranch => {
                self.open_git_picker(workspace_id, GitPickerPurpose::DeleteBranch, outcome);
            }
            _ => {}
        }
    }

    fn open_git_picker(
        &mut self,
        workspace_id: String,
        purpose: GitPickerPurpose,
        outcome: &mut ClientShellInput,
    ) {
        self.overlay = Some(ClientShellOverlay::GitPicker(ClientGitPickerOverlay {
            workspace_id: workspace_id.clone(),
            purpose,
            entries: Vec::new(),
            selected: 0,
            loading: true,
            error: None,
        }));
        outcome.repaint = true;
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitPickerList(crate::api::schema::GitPickerListParams {
                workspace_id,
                kind: purpose.kind(),
            }),
            PendingEndpointKind::GitPickerList,
            outcome,
        );
    }

    fn confirm_git_picker(&mut self, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::GitPicker(picker)) = self.overlay.take() else {
            return;
        };
        let Some(entry) = picker.entries.get(picker.selected) else {
            return;
        };
        let value = entry.value.clone();
        let method = match picker.purpose {
            // `stash pop` rather than `apply`: leaving an applied stash on the stack is a
            // common source of duplicate work later.
            GitPickerPurpose::ApplyStash => {
                crate::api::schema::Method::GitStashPop(crate::api::schema::GitStashPopParams {
                    workspace_id: picker.workspace_id,
                    stash_ref: value,
                })
            }
            GitPickerPurpose::SwitchBranch => crate::api::schema::Method::GitBranchSwitch(
                crate::api::schema::GitBranchNameParams {
                    workspace_id: picker.workspace_id,
                    name: value,
                },
            ),
            GitPickerPurpose::DeleteBranch => crate::api::schema::Method::GitBranchDelete(
                crate::api::schema::GitBranchNameParams {
                    workspace_id: picker.workspace_id,
                    name: value,
                },
            ),
        };
        self.push_endpoint_method(method, outcome);
        outcome.repaint = true;
    }

    /// Routes a key while the stash/branch picker is open. Returns whether the key was consumed.
    pub(super) fn route_git_picker_overlay_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(ClientShellOverlay::GitPicker(picker)) = self.overlay.as_mut() else {
            return false;
        };
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        match code {
            KeyCode::Esc if modifiers.is_empty() => {
                self.overlay = None;
                outcome.repaint = true;
                true
            }
            KeyCode::Up | KeyCode::Char('k') if modifiers.is_empty() => {
                picker.selected = picker.selected.saturating_sub(1);
                outcome.repaint = true;
                true
            }
            KeyCode::Down | KeyCode::Char('j') if modifiers.is_empty() => {
                picker.selected = picker
                    .selected
                    .saturating_add(1)
                    .min(picker.entries.len().saturating_sub(1));
                outcome.repaint = true;
                true
            }
            KeyCode::Enter if modifiers.is_empty() => {
                self.confirm_git_picker(outcome);
                true
            }
            _ => true,
        }
    }

    /// Opens the "new branch" text prompt, reusing the rename overlay's input machinery.
    fn open_git_branch_create_overlay(&mut self, workspace_id: String) {
        self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new branch",
            input: String::new(),
            replace_on_type: false,
            target: ClientRenameTarget::GitBranchCreate { workspace_id },
        }));
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
    fn ctrl_g_requests_commit_message_generation() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&ctrl_key(KeyCode::Char('g')), &mut outcome);

        assert!(consumed);
        assert!(state.git_panel.generating_commit_message);
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitCommitMessageGenerate(_)
        ));
    }

    #[test]
    fn commit_message_generation_replaces_an_existing_draft() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        state.git_panel.commit_message = "old draft".into();
        state.git_panel.generating_commit_message = true;

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitCommitMessageGenerate,
            Ok(
                crate::api::schema::ResponseResult::GitCommitMessageGenerated {
                    message: "fix: handle edge case".into(),
                },
            ),
        );

        assert!(repaint);
        assert!(!state.git_panel.generating_commit_message);
        assert_eq!(state.git_panel.commit_message, "fix: handle edge case");
        assert!(state.git_panel.last_error.is_none());
    }

    #[test]
    fn commit_message_generation_failure_keeps_the_draft_and_reports_the_error() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        state.git_panel.commit_message = "old draft".into();
        state.git_panel.generating_commit_message = true;

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitCommitMessageGenerate,
            Err(ClientShellEndpointError {
                code: Some("commit_message_generation_failed".into()),
                message: "\"claude\" produced no output".into(),
            }),
        );

        assert!(repaint);
        assert!(!state.git_panel.generating_commit_message);
        assert_eq!(state.git_panel.commit_message, "old draft");
        assert_eq!(
            state.git_panel.last_error.as_deref(),
            Some("\"claude\" produced no output")
        );
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
    fn esc_in_file_list_releases_focus_without_changing_the_tab() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.mode = ClientShellMode::SidebarGit;
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&key(KeyCode::Esc), &mut outcome);

        assert!(consumed);
        assert_eq!(state.mode, ClientShellMode::Terminal);
        assert_eq!(state.sidebar_view, SidebarSpacesView::Git);
    }

    #[test]
    fn esc_in_commit_box_releases_focus_without_changing_the_tab() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.mode = ClientShellMode::SidebarGit;
        state.git_panel.focus = GitSidebarFocus::CommitBox;
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&key(KeyCode::Esc), &mut outcome);

        assert!(consumed);
        assert_eq!(state.mode, ClientShellMode::Terminal);
        assert_eq!(state.sidebar_view, SidebarSpacesView::Git);
    }

    #[test]
    fn switching_away_from_the_git_tab_releases_panel_focus() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.mode = ClientShellMode::SidebarGit;
        let mut outcome = ClientShellInput::default();

        state.set_sidebar_view(SidebarSpacesView::Spaces, &mut outcome);

        assert_eq!(state.mode, ClientShellMode::Terminal);
    }

    #[test]
    fn collapsing_the_sidebar_releases_panel_focus() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.mode = ClientShellMode::SidebarGit;
        state.sidebar_collapsed = true;
        let mut outcome = ClientShellInput::default();

        state.release_sidebar_git_focus_if_hidden(&mut outcome);

        assert_eq!(state.mode, ClientShellMode::Terminal);
    }

    #[test]
    fn panel_focus_is_kept_while_the_tab_stays_visible() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.mode = ClientShellMode::SidebarGit;
        let mut outcome = ClientShellInput::default();

        state.release_sidebar_git_focus_if_hidden(&mut outcome);

        assert_eq!(state.mode, ClientShellMode::SidebarGit);
    }

    #[test]
    fn toggle_sidebar_git_view_from_spaces_shows_and_focuses_git() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.sidebar_view = SidebarSpacesView::Spaces;
        state.mode = ClientShellMode::Terminal;
        let mut outcome = ClientShellInput::default();

        state.toggle_sidebar_git_view(&mut outcome);

        assert_eq!(state.sidebar_view, SidebarSpacesView::Git);
        assert_eq!(state.mode, ClientShellMode::SidebarGit);
    }

    #[test]
    fn toggle_sidebar_git_view_refocuses_after_escape_instead_of_switching_to_spaces() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        // Git tab visible but unfocused, as left by Esc.
        state.sidebar_view = SidebarSpacesView::Git;
        state.mode = ClientShellMode::Terminal;
        let mut outcome = ClientShellInput::default();

        state.toggle_sidebar_git_view(&mut outcome);

        assert_eq!(state.sidebar_view, SidebarSpacesView::Git);
        assert_eq!(state.mode, ClientShellMode::SidebarGit);
    }

    #[test]
    fn toggle_sidebar_git_view_switches_to_spaces_when_already_focused() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.sidebar_view = SidebarSpacesView::Git;
        state.mode = ClientShellMode::SidebarGit;
        let mut outcome = ClientShellInput::default();

        state.toggle_sidebar_git_view(&mut outcome);

        assert_eq!(state.sidebar_view, SidebarSpacesView::Spaces);
        assert_eq!(state.mode, ClientShellMode::Terminal);
    }

    #[test]
    fn toggle_sidebar_git_view_does_not_focus_a_collapsed_sidebar() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.sidebar_view = SidebarSpacesView::Spaces;
        state.mode = ClientShellMode::Terminal;
        state.sidebar_collapsed = true;
        let mut outcome = ClientShellInput::default();

        state.toggle_sidebar_git_view(&mut outcome);

        assert_eq!(state.sidebar_view, SidebarSpacesView::Git);
        assert_eq!(state.mode, ClientShellMode::Terminal);
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

    #[test]
    fn enter_opens_the_diff_overlay_and_requests_it() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&key(KeyCode::Enter), &mut outcome);

        assert!(consumed);
        assert!(matches!(
            state.overlay,
            Some(ClientShellOverlay::GitDiff(_))
        ));
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitDiffGet(params)
                if params.path == "f.rs" && !params.staged
        ));
    }

    #[test]
    fn diff_response_fills_the_open_overlay() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.overlay = Some(ClientShellOverlay::GitDiff(
            crate::client::shell::state::ClientGitDiffOverlay {
                path: "f.rs".into(),
                staged: false,
                diff: None,
                scroll: 0,
                loading: true,
                error: None,
            },
        ));

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitDiffGet {
                path: "f.rs".into(),
                staged: false,
            },
            Ok(crate::api::schema::ResponseResult::GitFileDiff {
                diff: crate::api::schema::GitFileDiff::default(),
            }),
        );

        assert!(repaint);
        let Some(ClientShellOverlay::GitDiff(overlay)) = &state.overlay else {
            panic!("expected the diff overlay to still be open");
        };
        assert!(!overlay.loading);
        assert!(overlay.diff.is_some());
    }

    #[test]
    fn stale_diff_response_is_ignored_once_the_overlay_moved_on() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.overlay = Some(ClientShellOverlay::GitDiff(
            crate::client::shell::state::ClientGitDiffOverlay {
                path: "other.rs".into(),
                staged: false,
                diff: None,
                scroll: 0,
                loading: true,
                error: None,
            },
        ));

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitDiffGet {
                path: "f.rs".into(),
                staged: false,
            },
            Ok(crate::api::schema::ResponseResult::GitFileDiff {
                diff: crate::api::schema::GitFileDiff::default(),
            }),
        );

        assert!(!repaint);
        let Some(ClientShellOverlay::GitDiff(overlay)) = &state.overlay else {
            panic!("expected the diff overlay to still be open");
        };
        assert!(overlay.loading);
        assert!(overlay.diff.is_none());
    }

    #[test]
    fn esc_closes_the_diff_overlay() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.overlay = Some(ClientShellOverlay::GitDiff(
            crate::client::shell::state::ClientGitDiffOverlay {
                path: "f.rs".into(),
                staged: false,
                diff: None,
                scroll: 0,
                loading: false,
                error: None,
            },
        ));
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_diff_overlay_key(&key(KeyCode::Esc), &mut outcome);

        assert!(consumed);
        assert!(state.overlay.is_none());
    }

    #[test]
    fn m_key_opens_the_repo_command_menu() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_panel_key(&key(KeyCode::Char('m')), &mut outcome);

        assert!(consumed);
        let Some(ClientShellOverlay::ContextMenu(menu)) = &state.overlay else {
            panic!("expected the repo command menu to open");
        };
        assert!(matches!(
            &menu.target,
            ClientContextMenuTarget::GitRepo { workspace_id } if !workspace_id.is_empty()
        ));
        let labels: Vec<&str> = menu.items().iter().map(|item| item.label).collect();
        assert_eq!(
            labels,
            vec![
                "Fetch",
                "Pull",
                "Push",
                "View log",
                "Stash changes",
                "Apply stash...",
                "New branch...",
                "Switch branch...",
                "Delete branch...",
            ]
        );
    }

    #[test]
    fn fetch_action_sends_git_repo_fetch() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let workspace_id = state.focused_git_workspace_id().unwrap();
        let mut outcome = ClientShellInput::default();

        state.activate_git_repo_context_action(
            workspace_id,
            ClientContextMenuAction::GitFetch,
            &mut outcome,
        );

        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitRepoFetch(_)
        ));
    }

    #[test]
    fn new_branch_action_opens_the_rename_prompt() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        let workspace_id = state.focused_git_workspace_id().unwrap();
        let mut outcome = ClientShellInput::default();

        state.activate_git_repo_context_action(
            workspace_id,
            ClientContextMenuAction::GitNewBranch,
            &mut outcome,
        );

        assert!(matches!(
            &state.overlay,
            Some(ClientShellOverlay::Rename(rename))
                if matches!(rename.target, ClientRenameTarget::GitBranchCreate { .. })
        ));
    }

    #[test]
    fn confirming_new_branch_sends_git_branch_create_with_the_trimmed_name() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new branch",
            input: "  feature/x  ".into(),
            replace_on_type: false,
            target: ClientRenameTarget::GitBranchCreate {
                workspace_id: "w1".into(),
            },
        }));
        let mut outcome = ClientShellInput::default();

        state.save_rename_overlay(&mut outcome);

        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitBranchCreate(params) if params.name == "feature/x"
        ));
    }

    #[test]
    fn confirming_new_branch_with_an_empty_name_sends_nothing() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        state.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new branch",
            input: "   ".into(),
            replace_on_type: false,
            target: ClientRenameTarget::GitBranchCreate {
                workspace_id: "w1".into(),
            },
        }));
        let mut outcome = ClientShellInput::default();

        state.save_rename_overlay(&mut outcome);

        assert!(outcome.actions.is_empty());
    }

    fn open_stash_picker(state: &mut ClientShellState) -> ClientShellInput {
        let workspace_id = state.focused_git_workspace_id().unwrap();
        let mut outcome = ClientShellInput::default();
        state.activate_git_repo_context_action(
            workspace_id,
            ClientContextMenuAction::GitStashApply,
            &mut outcome,
        );
        outcome
    }

    #[test]
    fn apply_stash_action_requests_a_stash_picker() {
        let mut state = test_state_with_working_tree(one_unstaged_file());

        let outcome = open_stash_picker(&mut state);

        assert!(matches!(
            &state.overlay,
            Some(ClientShellOverlay::GitPicker(picker))
                if picker.purpose == GitPickerPurpose::ApplyStash
        ));
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitPickerList(params)
                if params.kind == crate::api::schema::GitPickerKind::Stash
        ));
    }

    #[test]
    fn picker_response_fills_entries_and_confirm_sends_stash_pop() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        open_stash_picker(&mut state);

        let repaint = state.handle_git_endpoint_result(
            PendingEndpointKind::GitPickerList,
            Ok(crate::api::schema::ResponseResult::GitPickerList {
                entries: vec![crate::api::schema::GitPickerEntry {
                    value: "stash@{0}".into(),
                    label: "stash@{0} WIP".into(),
                }],
            }),
        );
        assert!(repaint);

        let mut outcome = ClientShellInput::default();
        let consumed = state.route_git_picker_overlay_key(&key(KeyCode::Enter), &mut outcome);

        assert!(consumed);
        assert!(state.overlay.is_none());
        assert_eq!(outcome.actions.len(), 1);
        let ClientShellAction::Endpoint { request, .. } = &outcome.actions[0] else {
            panic!("expected an endpoint action");
        };
        assert!(matches!(
            &request.method,
            crate::api::schema::Method::GitStashPop(params) if params.stash_ref == "stash@{0}"
        ));
    }

    #[test]
    fn esc_closes_the_picker_overlay() {
        let mut state = test_state_with_working_tree(one_unstaged_file());
        open_stash_picker(&mut state);
        let mut outcome = ClientShellInput::default();

        let consumed = state.route_git_picker_overlay_key(&key(KeyCode::Esc), &mut outcome);

        assert!(consumed);
        assert!(state.overlay.is_none());
    }
}
