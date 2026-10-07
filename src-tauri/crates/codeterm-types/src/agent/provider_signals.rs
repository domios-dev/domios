use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub logged_in: bool,
    pub auth_method: Option<String>,
    pub subscription_type: Option<String>,
    pub config_directory: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorKind {
    RateLimit,
    Authentication,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "dontAsk")]
    DontAsk,
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionObservations {
    pub api_error: Option<ApiErrorKind>,
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub clears_api_error: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSignals {
    #[serde(default)]
    pub capabilities: Option<super::capability_probe::ProbeSnapshot>,
    #[serde(default)]
    pub background: super::background_work::BackgroundRoster,
    #[serde(skip)]
    pub auth_probe_pending: bool,
    pub auth: Option<AuthStatus>,
    pub api_error: Option<ApiErrorKind>,
    pub permission_mode: Option<PermissionMode>,
}

impl ProviderSignals {
    pub fn observe(&mut self, incoming: &SessionObservations) {
        if incoming.api_error.is_some() || incoming.clears_api_error {
            self.api_error = incoming.api_error;
        }
        if incoming.permission_mode.is_some() {
            self.permission_mode = incoming.permission_mode;
        }
    }
}
