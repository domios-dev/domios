use serde::{Deserialize, Serialize};
use ts_rs::TS;

impl super::Tab {
    /// The name this tab's own session declared for a runtime-qualified model id.
    /// Plain ids keep their catalogue rules, so they never read a declared name.
    pub fn declared_model_name(&self) -> Option<String> {
        let model = self
            .agent_session_model
            .as_deref()
            .or(self.agent_provider_resolved_model.as_deref())?;
        if !codeterm_types::pty::is_qualified_model_id(model) {
            return None;
        }
        let options = self
            .agent_runtime_state
            .as_ref()?
            .pointer("/controls/configOptions")?
            .as_array()?;
        options
            .iter()
            .filter(|option| {
                option.get("category").and_then(serde_json::Value::as_str) == Some("model")
                    || option.get("id").and_then(serde_json::Value::as_str) == Some("model")
            })
            .filter_map(|option| option.get("options")?.as_array())
            .flatten()
            .find(|value| value.get("value").and_then(serde_json::Value::as_str) == Some(model))?
            .get("name")?
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    }

    /// The session model as a message frame names it: never a raw qualified id.
    pub fn agent_session_model_label(&self) -> Option<String> {
        let model = self.agent_session_model.as_deref()?;
        Some(
            self.declared_model_name()
                .unwrap_or_else(|| codeterm_types::pty::plain_model_id(model)),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ModelIdentityMismatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub planned_model: Option<String>,
    pub requested_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provider_resolved_model: Option<String>,
    pub actual_model: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ModelFallbackNotice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub planned_model: Option<String>,
    pub requested_model: String,
    pub provider_resolved_model: String,
    #[serde(default)]
    pub actual_model: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionModelIdentity {
    pub session_id: String,
    pub session_binding_nonce: String,
    pub launch_report: Option<LaunchModelReport>,
    pub model_switch_turn_confirmed: bool,
}

/// The model a provider named when this binding started, with the transcript's
/// model at that instant: a transcript still naming it predates the launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchModelReport {
    pub model: String,
    pub transcript_model_at_launch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct PendingModelSwitchObservation {
    pub session_id: String,
    pub launched_model: String,
    pub transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary,
}
