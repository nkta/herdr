use crate::api::schema::{CommitAgentInfo, CommitAgentSetActiveParams, ResponseResult};
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_commit_agent_list(&self, id: String) -> String {
        let agents = self
            .state
            .commit_agents
            .iter()
            .map(|agent| CommitAgentInfo {
                id: agent.id.clone(),
                label: agent.label.clone(),
                command: agent.command.clone(),
            })
            .collect();
        encode_success(
            id,
            ResponseResult::CommitAgentList {
                agents,
                active: self.state.active_commit_agent.clone(),
            },
        )
    }

    pub(super) fn handle_commit_agent_set_active(
        &mut self,
        id: String,
        params: CommitAgentSetActiveParams,
    ) -> String {
        if !self
            .state
            .commit_agents
            .iter()
            .any(|agent| agent.id == params.id)
        {
            return encode_error(
                id,
                "unknown_commit_agent",
                format!("no commit agent is configured with id \"{}\"", params.id),
            );
        }
        if let Err(err) =
            crate::config::write_edit(crate::config::ConfigEdit::CommitAgentActive(&params.id))
        {
            return encode_error(id, "commit_agent_set_active_failed", err);
        }
        self.state.active_commit_agent = Some(params.id.clone());
        encode_success(
            id,
            ResponseResult::CommitAgentSetActive {
                active: self.state.active_commit_agent.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommitAgentConfig;

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
    fn list_reports_configured_agents_and_the_active_one() {
        let mut app = test_app();
        app.state.commit_agents = vec![CommitAgentConfig {
            id: "claude".into(),
            label: "Claude".into(),
            command: "claude".into(),
            args: vec!["-p".into()],
        }];
        app.state.active_commit_agent = Some("claude".into());

        let response = app.handle_commit_agent_list("req1".into());

        assert!(response.contains("\"claude\""));
        assert!(response.contains("\"active\":\"claude\""));
    }

    #[test]
    fn set_active_rejects_an_unknown_agent_id() {
        let mut app = test_app();
        app.state.commit_agents = Vec::new();
        app.state.active_commit_agent = None;

        let response = app.handle_commit_agent_set_active(
            "req1".into(),
            CommitAgentSetActiveParams {
                id: "missing".into(),
            },
        );

        assert!(response.contains("unknown_commit_agent"));
        assert_eq!(app.state.active_commit_agent, None);
    }

    #[test]
    fn set_active_persists_a_known_agent_id() {
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let unique = format!(
            "herdr-commit-agent-set-active-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique).join("config.toml");
        std::env::set_var(crate::config::CONFIG_PATH_ENV_VAR, &path);

        let mut app = test_app();
        app.state.commit_agents = vec![CommitAgentConfig {
            id: "codex".into(),
            label: "Codex".into(),
            command: "codex".into(),
            args: vec!["exec".into()],
        }];

        let response = app.handle_commit_agent_set_active(
            "req1".into(),
            CommitAgentSetActiveParams { id: "codex".into() },
        );

        assert!(response.contains("\"active\":\"codex\""));
        assert_eq!(app.state.active_commit_agent.as_deref(), Some("codex"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("[commit_agents]"));
        assert!(saved.contains("active = \"codex\""));

        std::env::remove_var(crate::config::CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
