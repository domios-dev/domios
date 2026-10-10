use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeModel {
    pub id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeAccount {
    pub subscription_type: Option<String>,
    pub api_provider: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeCapabilities {
    pub commands: Option<Vec<ProbeCommand>>,
    pub models: Option<Vec<ProbeModel>>,
    pub agents: Option<Vec<ProbeCommand>>,
    pub account: Option<ProbeAccount>,
    pub current_permission_mode: Option<String>,
    pub fast_mode_state: Option<String>,
    pub output_style: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeFailure {
    Disabled,
    Busy,
    Unsupported,
    BinaryUnavailable,
    ProjectUnavailable,
    Spawn,
    Protocol,
    Deadline,
    OutputLimit,
    CacheLimit,
    Cleanup,
    Superseded,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProbeSnapshot {
    Pending,
    Ready { data: InitializeCapabilities },
    Unavailable { reason: ProbeFailure },
}
