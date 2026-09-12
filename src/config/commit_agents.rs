use serde::Deserialize;

/// One external CLI agent that can be invoked to draft a git commit message from the staged
/// diff. `command` runs on the machine hosting the Herdr server (not necessarily the client's
/// machine), since that is where the git worktree and any agent binaries live.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommitAgentConfig {
    pub id: String,
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct CommitAgentsConfig {
    /// Id of the agent used by `git.commit_message.generate`. `None` disables generation.
    pub active: Option<String>,
    pub agents: Vec<CommitAgentConfig>,
}

impl Default for CommitAgentsConfig {
    fn default() -> Self {
        Self {
            active: Some("claude".to_string()),
            agents: vec![CommitAgentConfig {
                id: "claude".into(),
                label: "Claude".into(),
                command: "claude".into(),
                args: vec!["-p".into()],
            }],
        }
    }
}

impl CommitAgentsConfig {
    pub(crate) fn diagnostics(&self) -> Vec<String> {
        match &self.active {
            Some(id) if !self.agents.iter().any(|agent| &agent.id == id) => vec![format!(
                "commit_agents.active = \"{id}\" does not match any configured commit_agents.agents id; no commit agent is active"
            )],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn defaults_to_a_working_claude_agent() {
        let config = Config::default();
        assert_eq!(config.commit_agents.active.as_deref(), Some("claude"));
        assert_eq!(config.commit_agents.agents.len(), 1);
        assert_eq!(config.commit_agents.agents[0].id, "claude");
        assert_eq!(config.commit_agents.agents[0].command, "claude");
        assert_eq!(config.commit_agents.agents[0].args, vec!["-p".to_string()]);
    }

    #[test]
    fn parses_a_configured_agent_list() {
        let toml = r#"
[commit_agents]
active = "codex"

[[commit_agents.agents]]
id = "claude"
label = "Claude"
command = "claude"
args = ["-p"]

[[commit_agents.agents]]
id = "codex"
label = "Codex"
command = "codex"
args = ["exec"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.commit_agents.active.as_deref(), Some("codex"));
        assert_eq!(config.commit_agents.agents.len(), 2);
        assert_eq!(config.commit_agents.agents[1].id, "codex");
        assert_eq!(
            config.commit_agents.agents[1].args,
            vec!["exec".to_string()]
        );
    }

    #[test]
    fn missing_section_keeps_the_default() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.commit_agents, CommitAgentsConfig::default());
    }

    #[test]
    fn active_agent_not_in_the_list_produces_a_diagnostic() {
        let toml = r#"
[commit_agents]
active = "missing"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let diagnostics = config.commit_agents.diagnostics();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("commit_agents.active")
                && diagnostic.contains("missing")));
    }
}
