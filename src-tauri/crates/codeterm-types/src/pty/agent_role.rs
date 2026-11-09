use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use crate::agent::AgentRole;

/// An interactive prompt option (e.g. permission request).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct PromptOption {
    pub label: String,
    /// Value to send to PTY (e.g. "1", "y", "n")
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Provider,
    DirectoryTrust,
    Structured,
    Permission,
    AutomaticPermission,
    ManualUpdate,
}

impl PromptKind {
    pub fn is_structured(self) -> bool {
        matches!(
            self,
            Self::Structured | Self::Permission | Self::AutomaticPermission
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct PromptIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub request_id: Option<String>,
    pub kind: PromptKind,
    pub options: Vec<PromptOption>,
}

impl PromptIdentity {
    pub fn new(kind: PromptKind, prompt: &PendingPrompt) -> Self {
        Self {
            kind,
            request_id: prompt.request_id.clone(),
            options: prompt.options.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct PromptSection {
    pub label: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completed: bool,
}

/// A pending interactive prompt from an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct PendingPrompt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub request_id: Option<String>,
    pub question: String,
    pub options: Vec<PromptOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sections: Option<Vec<PromptSection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub multi_select: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub submit_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selected_values: Option<Vec<String>>,
}

#[cfg(test)]
mod tests;
