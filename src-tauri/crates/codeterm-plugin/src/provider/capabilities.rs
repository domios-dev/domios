//! Declared provider capability classes. These are the discriminants core
//! branches on — never a provider id — so a drop-in plugin reaches the same
//! code paths a native provider does.

use serde::{Deserialize, Serialize};

/// Which runtime drives a provider's tab: a PTY process or a JS chatBackend.
/// Replaces the old injected `kind` string and is the spawn discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTransport {
    Pty,
    ChatBackend,
}

/// What behavior surface a provider exposes for configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum BehaviorConfig {
    #[default]
    None,
    OwnPromptSet,
    PresetsAndSystemPrompt,
    Graph,
}

/// How a system prompt reaches the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPromptDelivery {
    CliFlag(String),
    SessionConfig,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSelfUpdateCapability {
    pub in_session: bool,
    pub restart_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_signal: Option<ProviderUpdateCompletionSignal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUpdateCompletionSignal {
    ProviderClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTrust {
    System,
    Unverified,
    ThirdParty,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProviderAllowanceCapability {
    pub default_reserve_pct: f32,
    pub telemetry_max_age_ms: u64,
}
