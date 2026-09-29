use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentityMismatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_model: Option<String>,
    pub requested_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_resolved_model: Option<String>,
    pub actual_model: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackNotice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingModelSwitchObservation {
    pub session_id: String,
    pub launched_model: String,
    pub transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary,
}
