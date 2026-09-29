//! Declared provider capability classes. These are the discriminants core
//! branches on — never a provider id — so a drop-in plugin reaches the same
//! code paths a native provider does.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SandboxedLinuxLaunchCapability {
    pub network_access_args: Vec<String>,
    pub network_access_config_path: String,
    pub network_access_setting: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogueAuthority {
    FreeForm,
    Authoritative,
}

impl Default for ModelCatalogueAuthority {
    fn default() -> Self {
        Self::FreeForm
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ModelCatalogueCapability {
    pub authority: ModelCatalogueAuthority,
    pub live_discovery_required: bool,
}

impl ModelCatalogueCapability {
    pub const fn authoritative() -> Self {
        Self {
            authority: ModelCatalogueAuthority::Authoritative,
            live_discovery_required: false,
        }
    }

    pub const fn authoritative_live() -> Self {
        Self {
            authority: ModelCatalogueAuthority::Authoritative,
            live_discovery_required: true,
        }
    }
}

/// How a live agent tab changes its model without touching the provider's
/// machine-global default. `RestartResume` relaunches the same session with the
/// model launch args and needs a session-start report naming the running model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSwitchMode {
    RestartResume,
    #[default]
    Unsupported,
}

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
    /// A provider reporting its own live turn state. Separate from `Stop`,
    /// which is a turn-completion notification with side effects.
    SessionState,
}

impl HookIngestionRoute {
    /// Resolve a declared route name against the closed set.
    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }
}

/// The structured live-state verdict a `session_state` event commits to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStateVerdict {
    Working,
    Idle,
}

impl ProviderStateVerdict {
    /// Resolve a declared verdict name against the closed set.
    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }
}

/// One declared hook event: the provider-side event name and the core
/// ingestion route its POSTs land on. `verdict` is meaningful only on the
/// `session_state` route, where it names the state the event commits to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct HookEventBinding {
    pub event: String,
    pub route: HookIngestionRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<ProviderStateVerdict>,
}

/// The verdict a declared roster commits an incoming event to. An undeclared
/// event, an event off the `session_state` route, or one declared without a
/// verdict all yield `None` — nothing changes.
pub fn declared_state_verdict(
    roster: &[HookEventBinding],
    event: &str,
) -> Option<ProviderStateVerdict> {
    let event = event.trim();
    roster
        .iter()
        .find(|binding| binding.event == event)
        .filter(|binding| binding.route == HookIngestionRoute::SessionState)
        .and_then(|binding| binding.verdict)
}

/// The minimal roster every hooked provider posts today: SessionStart is the
/// session-binding event a hook install carries even when nothing else is wired.
pub fn session_start_hook_roster() -> Vec<HookEventBinding> {
    vec![HookEventBinding {
        event: "SessionStart".into(),
        route: HookIngestionRoute::SessionStart,
        verdict: None,
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
    let Some(capability) = provider.integration_trust_grant_capability() else {
        return Ok(None);
    };
    let outcome = provider.grant_integration_trust(&capability.managed_keys);
    if outcome.get("granted").and_then(serde_json::Value::as_bool) != Some(true) {
        let reason = outcome
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .filter(|reason| !reason.trim().is_empty())
            .unwrap_or("provider did not confirm hook trust");
        return Err(format!("provider integration trust grant failed: {reason}"));
    }
    Ok(Some(outcome))
}

/// Marker prefix on a provider error meaning the plugin was denied a sandbox
/// capability during the call — it is confined and missing something it needs,
/// which is a different fact from "the upstream source failed". Callers match
/// on this constant, never on the sentence after it.
pub const CAPABILITY_DENIED_ERROR: &str = "capability_denied:";

/// Re-attribute a plugin call's failure when the sandbox denied a capability
/// while it ran. A confined provider that quietly returns nothing is the
/// failure mode that leaves a usage gauge frozen with nothing to explain it;
/// this is what gives that state a name the host can carry to the user.
pub fn attribute_capability_denial(
    outcome: Result<String, String>,
    denials_before: u64,
    denials_after: u64,
) -> Result<String, String> {
    match outcome {
        Ok(raw) => Ok(raw),
        Err(detail) if denials_after > denials_before => {
            Err(format!("{CAPABILITY_DENIED_ERROR} {detail}"))
        }
        Err(detail) => Err(detail),
    }
}

/// Whether a provider error carries the capability-denied marker.
pub fn is_capability_denied(error: &str) -> bool {
    error.starts_with(CAPABILITY_DENIED_ERROR)
}

#[cfg(test)]
mod capability_denial_tests {
    use super::*;

    #[test]
    fn a_failure_with_a_fresh_denial_is_reported_as_lost_capability() {
        let out = attribute_capability_denial(Err("fetchUsage returned null".into()), 3, 4);
        let err = out.expect_err("still a failure");
        assert!(
            is_capability_denied(&err),
            "a confined plugin's dead call must name itself: {err}"
        );
    }

    #[test]
    fn a_failure_without_a_denial_keeps_its_own_cause() {
        let out = attribute_capability_denial(Err("request timed out".into()), 3, 3);
        let err = out.expect_err("still a failure");
        assert!(
            !is_capability_denied(&err),
            "an upstream failure must not be blamed on confinement: {err}"
        );
    }

    #[test]
    fn a_success_is_never_rewritten_even_if_something_else_was_denied() {
        let out = attribute_capability_denial(Ok("weekly 7%".into()), 0, 9);
        assert_eq!(out.unwrap(), "weekly 7%");
    }
}
