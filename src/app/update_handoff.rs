//! Deciding when an installed update may replace the running server.
//!
//! Phase A (download + install) runs on the background update thread and ends
//! with `AppEvent::UpdateInstalled`. This module owns phase B: the headless
//! loop parks a [`PendingUpdateHandoff`] and, on a timer, checks
//! [`update_handoff_blocker`] — a pure read over `AppState` — before calling the
//! existing `perform_live_handoff`. The blocker keeps the handoff away from any
//! moment a coding agent is mid-task, where the brief client reconnect would
//! interrupt it.

use std::path::PathBuf;
use std::time::Duration;

use crate::app::state::{AppState, ToastKind, ToastNotification};
use crate::app::App;
use crate::detect::AgentState;

/// How long to wait before re-checking after the handoff was blocked.
pub(crate) const UPDATE_HANDOFF_RETRY_INTERVAL: Duration = Duration::from_secs(60);
/// Gap between the consecutive clear probes required before handing off.
pub(crate) const UPDATE_HANDOFF_SETTLE_INTERVAL: Duration = Duration::from_secs(15);
/// Consecutive clear probes required. Agent state can flicker to `Idle` at
/// tool-call boundaries, so one clear reading is not enough.
pub(crate) const UPDATE_HANDOFF_SETTLE_PROBES: u8 = 2;

/// An installed update waiting for a safe moment to take over the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingUpdateHandoff {
    pub(crate) version: String,
    pub(crate) exe_path: PathBuf,
    pub(crate) target_protocol: Option<u32>,
    /// Consecutive clear probes seen so far.
    pub(crate) clear_probes: u8,
    /// Whether the "blocked" reason has been logged at info once already.
    pub(crate) blocked_logged: bool,
}

impl PendingUpdateHandoff {
    pub(crate) fn new(version: String, exe_path: PathBuf, target_protocol: Option<u32>) -> Self {
        Self {
            version,
            exe_path,
            target_protocol,
            clear_probes: 0,
            blocked_logged: false,
        }
    }
}

/// Why the pending update handoff cannot run right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateHandoffBlocker {
    HandoffInProgress,
    AgentWorking,
    AgentLaunchPending,
    AgentResumePending,
    TooManyPanes,
}

impl UpdateHandoffBlocker {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::HandoffInProgress => "handoff already in progress",
            Self::AgentWorking => "an agent is working",
            Self::AgentLaunchPending => "an agent launch is pending",
            Self::AgentResumePending => "an agent resume is pending",
            Self::TooManyPanes => "too many panes for one handoff",
        }
    }
}

/// First blocking condition, or `None` when a live handoff is safe to start.
///
/// Pure over `AppState` plus the server's `handoff_in_progress` flag.
/// `Idle`, `Blocked`, and `Unknown` agents are all safe: PTYs are preserved
/// across the handoff, so a blocked prompt survives it untouched.
pub(crate) fn update_handoff_blocker(
    state: &AppState,
    handoff_in_progress: bool,
) -> Option<UpdateHandoffBlocker> {
    if handoff_in_progress {
        return Some(UpdateHandoffBlocker::HandoffInProgress);
    }

    if state
        .terminals
        .values()
        .any(|terminal| terminal.is_agent_terminal() && terminal.state == AgentState::Working)
    {
        return Some(UpdateHandoffBlocker::AgentWorking);
    }

    if state
        .terminals
        .values()
        .any(|terminal| terminal.managed_agent_launch_pending())
    {
        return Some(UpdateHandoffBlocker::AgentLaunchPending);
    }

    if state
        .terminals
        .values()
        .any(|terminal| terminal.pending_agent_resume_plan.is_some())
    {
        return Some(UpdateHandoffBlocker::AgentResumePending);
    }

    // The handoff limit is on distinct attached terminals, not raw pane count
    // (`perform_live_handoff` keys `pane_by_terminal` by `attached_terminal_id`).
    let attached: std::collections::HashSet<_> = state
        .workspaces
        .iter()
        .flat_map(|ws| ws.tabs.iter())
        .flat_map(|tab| tab.panes.values())
        .map(|pane| &pane.attached_terminal_id)
        .collect();
    if attached.len() > crate::server::handoff::MAX_FDS_PER_HANDOFF {
        return Some(UpdateHandoffBlocker::TooManyPanes);
    }

    None
}

impl App {
    /// Abandon a pending update handoff after it failed to complete.
    ///
    /// The install already landed on disk, so `update_available` stays set and
    /// the manual path still works — the user just has to relaunch Herdr.
    pub(crate) fn record_update_handoff_failure(&mut self, version: &str, err: &str) {
        crate::logging::update_handoff_failed(version, err);
        self.pending_update_handoff = None;
        self.next_update_handoff_attempt = None;

        if matches!(
            self.state.toast_config.delivery,
            crate::config::ToastDelivery::Herdr
        ) {
            let previous = self.state.toast.clone();
            self.state.toast = Some(ToastNotification {
                kind: ToastKind::UpdateInstalled,
                title: format!("v{version} handoff failed"),
                context: "restart Herdr when ready to use the new version".to_string(),
                position: None,
                target: None,
            });
            self.sync_toast_deadline(previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{TerminalId, TerminalState};

    fn agent_terminal(agent_state: AgentState) -> TerminalState {
        let mut terminal = TerminalState::new(TerminalId::alloc(), "/tmp".into());
        terminal.agent_name = Some("claude".to_string());
        terminal.state = agent_state;
        terminal
    }

    fn state_with_terminals(terminals: Vec<TerminalState>) -> AppState {
        let mut state = AppState::test_new();
        for terminal in terminals {
            state.terminals.insert(terminal.id.clone(), terminal);
        }
        state
    }

    #[test]
    fn blocker_is_clear_for_idle_and_blocked_agents() {
        let state = state_with_terminals(vec![
            agent_terminal(AgentState::Idle),
            agent_terminal(AgentState::Blocked),
            agent_terminal(AgentState::Unknown),
        ]);
        assert_eq!(update_handoff_blocker(&state, false), None);
    }

    #[test]
    fn blocker_reports_a_working_agent() {
        let state = state_with_terminals(vec![
            agent_terminal(AgentState::Idle),
            agent_terminal(AgentState::Working),
        ]);
        assert_eq!(
            update_handoff_blocker(&state, false),
            Some(UpdateHandoffBlocker::AgentWorking)
        );
    }

    #[test]
    fn blocker_ignores_working_state_on_a_non_agent_terminal() {
        let mut shell = TerminalState::new(TerminalId::alloc(), "/tmp".into());
        shell.state = AgentState::Working;
        let state = state_with_terminals(vec![shell]);
        assert_eq!(update_handoff_blocker(&state, false), None);
    }

    #[test]
    fn blocker_reports_handoff_in_progress_first() {
        let state = state_with_terminals(vec![agent_terminal(AgentState::Working)]);
        assert_eq!(
            update_handoff_blocker(&state, true),
            Some(UpdateHandoffBlocker::HandoffInProgress)
        );
    }

    #[test]
    fn blocker_reports_a_pending_agent_resume() {
        let mut terminal = agent_terminal(AgentState::Idle);
        terminal.pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "claude".to_string(),
            argv: vec!["--resume".to_string()],
            dedupe_key: "k".to_string(),
        });
        let state = state_with_terminals(vec![terminal]);
        assert_eq!(
            update_handoff_blocker(&state, false),
            Some(UpdateHandoffBlocker::AgentResumePending)
        );
    }

    #[test]
    fn blocker_reports_too_many_panes() {
        use ratatui::layout::Direction;

        let mut state = AppState::test_new();
        let mut ws = crate::workspace::Workspace::test_new("many");
        for _ in 0..crate::server::handoff::MAX_FDS_PER_HANDOFF {
            ws.test_split(Direction::Vertical);
        }
        state.workspaces.push(ws);
        assert_eq!(
            update_handoff_blocker(&state, false),
            Some(UpdateHandoffBlocker::TooManyPanes)
        );
    }
}
