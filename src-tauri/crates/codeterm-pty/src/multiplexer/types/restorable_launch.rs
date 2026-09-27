use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningEffortIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_source: Option<String>,
}

impl ReasoningEffortIdentity {
    pub fn is_empty(&self) -> bool {
        self.planned.is_none()
            && self.requested.is_none()
            && self.resolved.is_none()
            && self.actual.is_none()
            && self.actual_source.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorableLaunchParameters {
    #[serde(default)]
    pub captured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_instructions: Option<String>,
    #[serde(default)]
    pub permission_bypass_requested: bool,
}

impl RestorableLaunchParameters {
    pub fn is_empty(&self) -> bool {
        !self.captured
            && self.requested_model.is_none()
            && self.model.is_none()
            && self.requested_reasoning_effort.is_none()
            && self.resolved_reasoning_effort.is_none()
            && self.args.is_none()
            && self.agent_name.is_none()
            && self.identity_instructions.is_none()
            && !self.permission_bypass_requested
    }
}
