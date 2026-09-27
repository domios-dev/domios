use serde::{Deserialize, Serialize};

pub use crate::agent::AgentRole;

/// An interactive prompt option (e.g. permission request).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptOption {
    pub label: String,
    /// Value to send to PTY (e.g. "1", "y", "n")
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Provider,
    DirectoryTrust,
    Structured,
    ManualUpdate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptIdentity {
    pub kind: PromptKind,
    pub options: Vec<PromptOption>,
}

impl PromptIdentity {
    pub fn new(kind: PromptKind, prompt: &PendingPrompt) -> Self {
        Self {
            kind,
            options: prompt.options.clone(),
        }
    }
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
