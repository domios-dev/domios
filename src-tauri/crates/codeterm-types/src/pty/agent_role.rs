use serde::{Deserialize, Serialize};

/// Agent role in the orchestration hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AgentRole {
    /// Top-level orchestrator (depth 1). Can spawn Managers and Workers.
    Orchestrator,
    /// Manager (depth 2). Specialized by role_profile. Cannot spawn in v1.
    Manager,
    /// Worker (depth 2). Executes tasks, reports back. Cannot spawn.
    Worker,
}

impl AgentRole {
    pub fn from_str_compat(s: &str) -> Option<Self> {
        match s {
            "Orchestrator" | "Conductor" => Some(Self::Orchestrator),
            "Manager" => Some(Self::Manager),
            "Worker" => Some(Self::Worker),
            _ => None,
        }
    }
}

/// An interactive prompt option (e.g. permission request).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptOption {
    pub label: String,
    /// Value to send to PTY (e.g. "1", "y", "n")
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptSection {
    pub label: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completed: bool,
}

/// A pending interactive prompt from an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingPrompt {
    pub question: String,
    pub options: Vec<PromptOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<PromptSection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_values: Option<Vec<String>>,
}
