#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteTaskCapability {
    pub backend: String,
    pub local_provider_id: String,
    pub availability: RemoteTaskAvailability,
    pub environments: Vec<RemoteEnvironment>,
    pub follow_up: bool,
    pub cancel: bool,
    pub apply_diff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteTaskAvailability {
    Available,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteEnvironment {
    pub id: String,
    pub label: String,
    pub account_ref: String,
}
