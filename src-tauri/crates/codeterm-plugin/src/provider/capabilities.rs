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

/// Who composes the conversational context a backend sends: the provider itself
/// (native, today's behaviour everywhere) or the host, which hands over a turn
/// it composed and recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextOwner {
    #[default]
    Plugin,
    Host,
}

impl ContextOwner {
    /// Resolve a declared value against the closed set. `None` ⇒ the caller
    /// fails the parse; an unknown string must never resolve to `Plugin`.
    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }

    pub fn is_plugin(&self) -> bool {
        matches!(self, Self::Plugin)
    }
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

/// Structured rejection when a requested reasoning effort is not supported by a
/// provider. Carries the exact supported set so callers surface an honest error
/// instead of silently downgrading. Serializable for API/CLI/report payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ReasoningEffortRejection {
    pub provider: String,
    pub requested: String,
    pub supported: Vec<String>,
}

impl std::fmt::Display for ReasoningEffortRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.supported.is_empty() {
            write!(
                f,
                "provider '{}' does not support reasoning-effort selection (requested '{}')",
                self.provider, self.requested
            )
        } else {
            write!(
                f,
                "provider '{}' does not support reasoning effort '{}'; supported: {}",
                self.provider,
                self.requested,
                self.supported.join(", ")
            )
        }
    }
}

impl std::error::Error for ReasoningEffortRejection {}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProviderAllowanceCapability {
    pub default_reserve_pct: f32,
    pub telemetry_max_age_ms: u64,
}

/// Core hook-ingestion endpoints a declared hook event may target. Closed
/// set: a roster naming any other route fails registration at install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookIngestionRoute {
    Permission,
    Stop,
    SessionStart,
    MemFlush,
}

impl HookIngestionRoute {
    /// Resolve a declared route name against the closed set.
    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }
}

/// One declared hook event: the provider-side event name and the core
/// ingestion route its POSTs land on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct HookEventBinding {
    pub event: String,
    pub route: HookIngestionRoute,
}

/// The minimal roster every hooked provider posts today: SessionStart is the
/// session-binding event a hook install carries even when nothing else is wired.
pub fn session_start_hook_roster() -> Vec<HookEventBinding> {
    vec![HookEventBinding {
        event: "SessionStart".into(),
        route: HookIngestionRoute::SessionStart,
    }]
}

/// Optional `integration.trust_grant` capability: after integration install,
/// core invokes the provider-owned grant exactly once with these managed hook
/// key(s). The provider's own CLI is the only trust-hash authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IntegrationTrustGrantCapability {
    pub managed_keys: Vec<String>,
}

/// Core's install seam: install, then invoke a declared trust grant exactly
/// once with the declared managed key(s). `Ok(None)` ⇒ no capability declared.
pub fn install_integration_with_trust_grant(
    provider: &dyn super::AgentProvider,
    api_port: u16,
    options: &super::IntegrationInstallOptions,
) -> Result<Option<serde_json::Value>, String> {
    provider.install_integration(api_port, options)?;
    Ok(provider
        .integration_trust_grant_capability()
        .map(|capability| provider.grant_integration_trust(&capability.managed_keys)))
}
