use serde::{Deserialize, Serialize};

/// One configured commit-message-generation agent, as exposed to a client (the invocation
/// `args` stay server-only — a client only needs enough to label a choice).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommitAgentInfo {
    pub id: String,
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommitAgentSetActiveParams {
    pub id: String,
}
