use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{
    state::{GitSidebarFocus, Mode},
    App,
};
use crate::events::GitFileAction;
use crate::workspace::GitFileStatusKind;

impl App {
    /// Handles a key while `Mode::SidebarGit` has focus (the Git panel's file list or its commit
    /// message box). `Tab` toggles focus between the two — this is local to the mode's own key
    /// dispatch, distinct from the global (configurable) SPACES/GIT tab-strip switch, so it
    /// cannot conflict with it.
    pub(crate) fn handle_sidebar_git_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.leave_sidebar_git_mode();
            return;
        }

        if key.code == KeyCode::Tab {
            self.state.git_sidebar.focus = match self.state.git_sidebar.focus {
                GitSidebarFocus::FileList => GitSidebarFocus::CommitBox,
                GitSidebarFocus::CommitBox => GitSidebarFocus::FileList,
            };
            return;
        }

        match self.state.git_sidebar.focus {
            GitSidebarFocus::FileList => self.handle_git_file_list_key(key),
            GitSidebarFocus::CommitBox => self.handle_git_commit_box_key(key),
        }
    }

    fn leave_sidebar_git_mode(&mut self) {
        // Esc unwinds one step at a time: dismiss a pending discard, then close the diff panel,
        // and only then hand the keyboard back to the terminal.
        if self.state.git_sidebar.pending_discard.take().is_some() {
            return;
        }
        if self.state.git_diff_view.is_some() {
            self.state.git_diff_view = None;
            return;
        }
        self.state.mode = if self.state.active.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };
    }

    fn handle_git_file_list_key(&mut self, key: KeyEvent) {
        if self.state.git_sidebar.pending_discard.is_some() {
            match key.code {
                KeyCode::Char('y' | 'Y') => self.confirm_pending_discard(),
                _ => self.state.git_sidebar.pending_discard = None,
            }
            return;
        }

        let row_count = self.state.git_sidebar.rows().len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.state.git_sidebar.selected.move_prev(),
            KeyCode::Down | KeyCode::Char('j') => {
                self.state.git_sidebar.selected.move_next(row_count)
            }
            KeyCode::Enter => self.open_selected_git_diff(),
            KeyCode::Char('s') => self.stage_selected(),
            KeyCode::Char('u') => self.unstage_selected(),
            KeyCode::Char('d') => self.request_discard_selected(),
            _ => {}
        }
    }

    fn handle_git_commit_box_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_git_commit();
            }
            KeyCode::Enter => self.state.git_sidebar.commit_message.push('\n'),
            KeyCode::Backspace => {
                self.state.git_sidebar.commit_message.pop();
            }
            KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                self.state.git_sidebar.commit_message.push(c);
            }
            _ => {}
        }
    }

    fn open_selected_git_diff(&mut self) {
        let Some(repo_root) = self.git_panel_repo_root() else {
            return;
        };
        let Some((entry, staged)) = self.state.git_sidebar.selected_row() else {
            return;
        };
        let untracked = entry.status == GitFileStatusKind::Untracked;
        self.open_git_diff(repo_root, entry.path.clone(), staged, untracked);
    }

    fn stage_selected(&mut self) {
        let Some(repo_root) = self.git_panel_repo_root() else {
            return;
        };
        let Some((entry, staged)) = self.state.git_sidebar.selected_row() else {
            return;
        };
        if staged {
            return;
        }
        self.start_git_file_action(repo_root, entry.path.clone(), GitFileAction::Stage);
    }

    fn unstage_selected(&mut self) {
        let Some(repo_root) = self.git_panel_repo_root() else {
            return;
        };
        let Some((entry, staged)) = self.state.git_sidebar.selected_row() else {
            return;
        };
        if !staged {
            return;
        }
        self.start_git_file_action(repo_root, entry.path.clone(), GitFileAction::Unstage);
    }

    fn request_discard_selected(&mut self) {
        let Some((entry, staged)) = self.state.git_sidebar.selected_row() else {
            return;
        };
        // Discard only applies to unstaged working-tree edits: staged changes must be unstaged
        // first, and `git restore` cannot remove an untracked file at all.
        if staged || entry.status == GitFileStatusKind::Untracked {
            return;
        }
        self.state.git_sidebar.pending_discard = Some(entry.path.clone());
    }

    fn confirm_pending_discard(&mut self) {
        let Some(path) = self.state.git_sidebar.pending_discard.take() else {
            return;
        };
        let Some(repo_root) = self.git_panel_repo_root() else {
            return;
        };
        self.start_git_file_action(repo_root, path, GitFileAction::Discard);
    }

    pub(crate) fn submit_git_commit(&mut self) {
        let message = self.state.git_sidebar.commit_message.trim().to_string();
        if message.is_empty() || self.state.git_sidebar.commit_in_flight {
            return;
        }
        let Some(repo_root) = self.git_panel_repo_root() else {
            return;
        };
        self.start_git_commit(repo_root, message);
    }
}
