use crate::app::git_commands::GitRepoCommand;
use crate::app::state::Mode;
use crate::app::App;
use crate::events::GitFileAction;

use super::modal::leave_modal;

impl App {
    /// Applies an item picked from the Git panel's per-file context menu.
    pub(super) fn apply_git_file_menu_action(
        &mut self,
        path: String,
        staged: bool,
        untracked: bool,
        action: &str,
    ) {
        leave_modal(&mut self.state);
        let Some(repo_root) = self.git_panel_repo_root() else {
            self.state.git_sidebar.last_error = Some("no git repository for this space".into());
            return;
        };

        match action {
            "Open diff" => self.open_git_diff(repo_root, path, staged, untracked),
            "Stage" => self.start_git_file_action(repo_root, path, GitFileAction::Stage),
            "Unstage" => self.start_git_file_action(repo_root, path, GitFileAction::Unstage),
            "Discard changes..." => {
                // Destructive: route through the same inline confirmation the `d` key uses rather
                // than discarding straight from the menu.
                self.state.git_sidebar.pending_discard = Some(path);
                self.state.mode = Mode::SidebarGit;
                self.state.git_sidebar.focus = crate::app::state::GitSidebarFocus::FileList;
            }
            _ => {}
        }
    }

    /// Applies an item picked from the Git panel's repository-wide context menu.
    pub(super) fn apply_git_repo_menu_action(&mut self, action: &str) {
        leave_modal(&mut self.state);

        match action {
            "Fetch" => self.run_git_repo_command(GitRepoCommand::Fetch),
            "Pull" => self.run_git_repo_command(GitRepoCommand::Pull),
            "Push" => self.run_git_repo_command(GitRepoCommand::Push),
            "View log" => self.run_git_repo_command(GitRepoCommand::Log),
            "Stash changes" => self.run_git_repo_command(GitRepoCommand::StashPush),
            "Apply stash..." => self.open_git_stash_picker(),
            "New branch..." => self.open_git_branch_create(),
            "Switch branch..." => self.open_git_branch_picker(),
            "Delete branch..." => self.open_git_branch_delete_picker(),
            _ => {}
        }
    }
}
