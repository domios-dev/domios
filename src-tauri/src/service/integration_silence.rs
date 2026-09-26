//! A provider that registers no sessions says so at launch.
//!
//! `agent_session_id` is written only by a provider's integration hook, and
//! installing that hook needs explicit consent. Without it nothing registers
//! and, until now, nothing said so — the gap surfaced hours later as a refused
//! plan write. The check here is read state: a declared capability plus the
//! install and consent records the daemon already holds. It never infers the
//! gap from an absence of sessions, which would need a window to decide in.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::agent::registry::AgentRegistry;
use crate::notification::dedupe::{
    provider_integration_missing_condition, ConditionTransition, MACHINE_SCOPE,
};
use crate::session::lifecycle::BindingSource;
use codeterm_types::notification::NotifyLevel;

use super::notification::{NotificationOrigin, PushIntent};
use super::ServiceContext;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
struct HookBindingFallback {
    source: BindingSource,
}

type HookBindingFallbacks = HashMap<String, HashMap<String, HookBindingFallback>>;

fn hook_binding_fallbacks() -> &'static parking_lot::Mutex<HookBindingFallbacks> {
    static FALLBACKS: OnceLock<parking_lot::Mutex<HookBindingFallbacks>> = OnceLock::new();
    FALLBACKS.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

pub(crate) fn binding_source_label(source: BindingSource) -> &'static str {
    match source {
        BindingSource::Hook => "hook",
        BindingSource::PidRegistry => "pid_registry",
        BindingSource::LaunchMarker => "launch_marker",
    }
}

fn update_hook_binding_fallback(
    fallbacks: &mut HashMap<String, HookBindingFallback>,
    provider_id: &str,
    session_id: &str,
    source: BindingSource,
    session_binding_hook_succeeded: bool,
) -> bool {
    if session_binding_hook_succeeded && source == BindingSource::Hook {
        let remove_provider = fallbacks
            .get_mut(provider_id)
            .is_some_and(|sessions| {
                sessions.remove(session_id);
                sessions.is_empty()
            });
        if remove_provider {
            fallbacks.remove(provider_id);
        }
        return false;
    }
    if source == BindingSource::Hook {
        return false;
    }
    let fallback = HookBindingFallback {
        source,
    };
    fallbacks
        .entry(provider_id.to_string())
        .or_default()
        .insert(session_id.to_string(), fallback.clone())
        != Some(fallback)
}

pub(crate) fn record_hook_binding_source(
    registry: &AgentRegistry,
    provider_id: &str,
    session_id: &str,
    source: BindingSource,
    session_binding_hook_succeeded: bool,
) {
    let has_hooks = registry
        .with_provider(provider_id, |provider| provider.has_hooks())
        .unwrap_or(false);
    if !has_hooks {
        return;
    }
    let changed = {
        let mut fallbacks = hook_binding_fallbacks().lock();
        update_hook_binding_fallback(
            &mut fallbacks,
            provider_id,
            session_id,
            source,
            session_binding_hook_succeeded,
        )
    };
    if changed {
        log::warn!(
            "[provider-integration] session binding used fallback source={} provider={}",
            binding_source_label(source),
            provider_id,
        );
    }
}

pub(crate) fn hook_binding_fallback_source(
    provider_id: &str,
    live_session_ids: &[String],
) -> Option<String> {
    hook_binding_fallback_source_from(
        &hook_binding_fallbacks().lock(),
        provider_id,
        live_session_ids,
    )
}

fn hook_binding_fallback_source_from(
    fallbacks: &HookBindingFallbacks,
    provider_id: &str,
    live_session_ids: &[String],
) -> Option<String> {
    let sessions = fallbacks.get(provider_id)?;
    let mut active_fallbacks = live_session_ids
        .iter()
        .filter_map(|session_id| sessions.get(session_id).map(|fallback| (session_id, fallback)))
        .collect::<Vec<_>>();
    active_fallbacks.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    active_fallbacks
        .first()
        .map(|(_, fallback)| binding_source_label(fallback.source).to_string())
}

pub(crate) fn has_hook_binding_fallback(provider_id: &str, session_id: &str) -> bool {
    hook_binding_fallbacks()
        .lock()
        .get(provider_id)
        .is_some_and(|sessions| sessions.contains_key(session_id))
}

pub(crate) fn integration_status_with_binding_fallback(
    status: Json,
    provider_id: &str,
    has_hooks: bool,
    live_session_ids: &[String],
) -> Json {
    let source = if has_hooks {
        hook_binding_fallback_source(provider_id, live_session_ids)
    } else {
        None
    };
    integration_status_with_fallback_source(status, has_hooks, source)
}

fn integration_status_with_fallback_source(
    mut status: Json,
    has_hooks: bool,
    source: Option<String>,
) -> Json {
    if let Some(fields) = status.as_object_mut() {
        fields.remove("codeterm_binding_fallback_source");
        if let (true, Some(source)) = (has_hooks, source) {
            fields.insert(
                "codeterm_binding_fallback_source".into(),
                Json::String(source),
            );
        }
    }
    status
}

/// What the daemon knows outright about one provider's session-registering
/// integration. Every field is a record readable now, none is an observation
/// accumulated over time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationFacts {
    /// The provider declares that it reports its sessions through hooks. The
    /// whole mechanism keys on this, never on which provider it is.
    pub registers_sessions_via_hooks: bool,
    /// The provider's own report of its install state. `None` when it reports
    /// nothing this reading understands.
    pub installed: Option<bool>,
    /// Consent recorded by the install path, which refuses without it.
    pub consent_recorded: bool,
    /// Where the provider says its integration lives.
    pub target: Option<String>,
    pub state: Option<IntegrationState>,
    /// The provider's own hash of the integration content it is judging. Two
    /// instances writing byte-identical content report the same value, so the
    /// verdict cannot key on which of them wrote it.
    pub content_hash: Option<String>,
    /// The provider's own report that the installed hooks carry its current
    /// ownership contract. `None` when it publishes no such field; `Some(false)`
    /// is a working but pre-contract install, which reconciliation migrates and
    /// the announcement stays quiet about.
    pub contract_current: Option<bool>,
}

/// The observed state one announcement covered, remembered across restarts.
/// The in-memory condition latch cannot do this: it is rebuilt empty on every
/// daemon start, so an unchanged hook re-announced on each one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncedIntegrationState {
    pub state: String,
    #[serde(default)]
    pub content_hash: Option<String>,
}

type AnnouncedMap = std::collections::HashMap<String, AnnouncedIntegrationState>;

pub fn announced_path() -> std::path::PathBuf {
    crate::paths::data_dir().join("integration-announced.json")
}

/// Authoritative in memory, durable on disk. Read-modify-write straight across
/// the shared file is a lost update: two launches racing here truncated each
/// other and left JSON that then read as "nothing was ever announced".
fn announced() -> &'static parking_lot::Mutex<AnnouncedMap> {
    static ANNOUNCED: std::sync::OnceLock<parking_lot::Mutex<AnnouncedMap>> =
        std::sync::OnceLock::new();
    ANNOUNCED.get_or_init(|| parking_lot::Mutex::new(load_announced()))
}

fn load_announced() -> AnnouncedMap {
    let path = announced_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return AnnouncedMap::new();
    };
    match serde_json::from_str(&raw) {
        Ok(map) => map,
        // Unreadable is not "nothing was announced". Announcing a standing gap
        // twice is recoverable; silently dropping it is the bug being fixed.
        Err(error) => {
            log::warn!(
                "[integration-silence] {} is unreadable ({error}) - every standing gap announces once more",
                path.display()
            );
            AnnouncedMap::new()
        }
    }
}

/// Renamed into place under the lock that owns the map, so neither a reader nor
/// a crash can observe the half-written file this used to leave behind.
fn persist_announced(all: &AnnouncedMap) {
    let path = announced_path();
    let Ok(json) = serde_json::to_string_pretty(all) else {
        return;
    };
    let staged = path.with_extension("json.tmp");
    if let Err(error) = std::fs::write(&staged, json).and_then(|()| std::fs::rename(&staged, &path))
    {
        log::warn!(
            "[integration-silence] could not record announcement state at {}: {error}",
            path.display()
        );
        let _ = std::fs::remove_file(&staged);
    }
}

/// News only when the observed state differs from the one already announced.
/// A restart that observes the same gap is the same news, not new news.
pub fn is_news(provider_id: &str, observed: &AnnouncedIntegrationState) -> bool {
    announced().lock().get(provider_id) != Some(observed)
}

pub fn record_announced(provider_id: &str, observed: &AnnouncedIntegrationState) {
    let mut all = announced().lock();
    all.insert(provider_id.to_string(), observed.clone());
    persist_announced(&all);
}

/// The gap closed, so the next one is news again.
pub fn clear_announced(provider_id: &str) {
    let mut all = announced().lock();
    if all.remove(provider_id).is_some() {
        persist_announced(&all);
    }
}

impl AnnouncedIntegrationState {
    pub fn observed(state: IntegrationState, facts: &IntegrationFacts) -> Self {
        Self {
            state: state.as_str().to_string(),
            content_hash: facts.content_hash.clone(),
        }
    }
}

pub const INTEGRATION_STATE_ABSENT: &str = "absent";
pub const INTEGRATION_STATE_STALE_VERSION: &str = "stale_version";
pub const INTEGRATION_STATE_UNTRUSTED_HASH: &str = "untrusted_hash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationState {
    Absent,
    StaleVersion,
    UntrustedHash,
    Installed,
}

impl IntegrationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => INTEGRATION_STATE_ABSENT,
            Self::StaleVersion => INTEGRATION_STATE_STALE_VERSION,
            Self::UntrustedHash => INTEGRATION_STATE_UNTRUSTED_HASH,
            Self::Installed => "installed",
        }
    }
}

/// The next step the message names for the state the provider reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationRemedy {
    /// Nothing was ever consented to, so granting consent is the whole repair.
    GrantConsentAndInstall {
        target: Option<String>,
    },
    /// Consent stands and the hooks are gone, so the same path reinstalls them.
    ReinstallIntegration {
        target: Option<String>,
    },
    RefreshIntegration {
        target: Option<String>,
    },
    GrantHookTrust {
        target: Option<String>,
    },
}

impl IntegrationRemedy {
    /// Named by constant so a surface can render it and a test can assert it
    /// without either one quoting a sentence.
    pub const INSTALL_PATH: &'static str = "Settings > Integrations > Install integration";
    pub const REFRESH_PATH: &'static str = "Settings > Integrations > Refresh integration";
    pub const TRUST_PATH: &'static str = "Settings > Integrations > Grant hook trust";

    pub fn next_step(&self) -> &'static str {
        match self {
            Self::GrantConsentAndInstall { .. } | Self::ReinstallIntegration { .. } => {
                Self::INSTALL_PATH
            }
            Self::RefreshIntegration { .. } => Self::REFRESH_PATH,
            Self::GrantHookTrust { .. } => Self::TRUST_PATH,
        }
    }

    pub fn target(&self) -> Option<&str> {
        match self {
            Self::GrantConsentAndInstall { target }
            | Self::ReinstallIntegration { target }
            | Self::RefreshIntegration { target }
            | Self::GrantHookTrust { target } => target.as_deref(),
        }
    }
}

/// One launch-time announcement, before any surface renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationSilence {
    pub provider_id: String,
    pub level: NotifyLevel,
    pub remedy: IntegrationRemedy,
    pub state: IntegrationState,
    /// The standing condition this announces, and the latch that keeps a
    /// second launch quiet.
    pub condition: String,
}

impl IntegrationSilence {
    pub fn title(&self, provider_display_name: &str) -> String {
        format!(
            "{provider_display_name} registers no sessions ({})",
            self.state.as_str()
        )
    }

    /// The standing explanation, named by constant so the record and the words
    /// the user reads cannot disagree.
    pub const EXPLANATION: &'static str =
        "This provider reports its sessions through an integration that is not ready, so nothing launched here is bound to a session.";

    /// Composed from the remedy, so the message always names a next step.
    pub fn body(&self) -> String {
        let mut lines = vec![
            Self::EXPLANATION.to_string(),
            format!("Integration state: {}", self.state.as_str()),
            format!("Next: {}", self.remedy.next_step()),
        ];
        if let Some(target) = self.remedy.target() {
            lines.push(format!("Installs into: {target}"));
        }
        lines.join("\n")
    }

    /// The declared level is the whole routing decision the user configures;
    /// the condition doubles as the coalescing key.
    pub fn intent(&self) -> PushIntent {
        PushIntent {
            level: Some(self.level),
            event_key: Some(self.condition.clone()),
        }
    }
}

/// What one reading of the facts concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchVerdict {
    /// The provider reports sessions through an integration that is not in
    /// place. The user has to act, so this is what announces.
    Silent(IntegrationSilence),
    /// The integration is in place, or the provider needs none. The latch is
    /// released so a later regression is news again.
    Registering,
    /// The provider reports nothing this reading understands. Saying nothing is
    /// the honest answer; concluding anything else would need a window.
    Unreadable,
}

/// The provider reports install state as opaque JSON, so the reading is the
/// closed precedence the settings health card already uses — session hook
/// first, because that is the hook that registers a session.
pub fn read_installed(status: &Json) -> Option<bool> {
    if let Some(session_start) = status.get("session_start_hook").and_then(Json::as_bool) {
        return Some(session_start);
    }
    if let Some(hooks_installed) = status.get("hooks_installed").and_then(Json::as_bool) {
        return Some(hooks_installed);
    }
    if let Some(plugin_installed) = status.get("plugin_installed").and_then(Json::as_bool) {
        let current = status.get("plugin_file_current").and_then(Json::as_bool) != Some(false);
        return Some(plugin_installed && current);
    }
    None
}

pub fn read_state(status: &Json, installed: Option<bool>) -> Option<IntegrationState> {
    if let Some(state) = status.get("integration_state").and_then(Json::as_str) {
        return match state {
            INTEGRATION_STATE_ABSENT => Some(IntegrationState::Absent),
            INTEGRATION_STATE_STALE_VERSION => Some(IntegrationState::StaleVersion),
            INTEGRATION_STATE_UNTRUSTED_HASH => Some(IntegrationState::UntrustedHash),
            "installed" => Some(IntegrationState::Installed),
            _ => None,
        };
    }
    match installed {
        Some(true) => Some(IntegrationState::Installed),
        Some(false) if status.get("hooks_owned").and_then(Json::as_bool) == Some(true) => {
            match status.get("hook_trust_state").and_then(Json::as_str) {
                Some("disabled") | Some("missing") | Some("unknown") => {
                    Some(IntegrationState::UntrustedHash)
                }
                _ => Some(IntegrationState::StaleVersion),
            }
        }
        Some(false) => Some(IntegrationState::Absent),
        None => None,
    }
}

/// The provider's own content hash, when it publishes one. Absent is honest:
/// a provider that hashes nothing gets a state-only latch, never a guess.
pub fn read_content_hash(status: &Json) -> Option<String> {
    status
        .get("hook_content_hash")
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .map(str::to_string)
}

/// The provider's own contract-currency report. Absent is honest: a provider
/// that declares no contract gets no migration verdict, never a guess.
pub fn read_contract_current(status: &Json) -> Option<bool> {
    status.get("hook_contract_current").and_then(Json::as_bool)
}

pub fn read_integration_owned(status: &Json) -> bool {
    status
        .get("integration_owned")
        .or_else(|| status.get("hooks_owned"))
        .or_else(|| status.get("plugin_file_owned"))
        .and_then(Json::as_bool)
        .unwrap_or(false)
}

pub fn read_target(status: &Json) -> Option<String> {
    status
        .get("target")
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(str::to_string)
}

/// Read one provider's declared capability and its own install report. The
/// consent record is the caller's to supply — it lives in config, not here.
pub fn read_provider_facts(
    registry: &AgentRegistry,
    provider_id: &str,
    consent_recorded: bool,
) -> Option<(IntegrationFacts, String)> {
    let (registers_sessions_via_hooks, status, display_name) =
        registry.with_provider(provider_id, |provider| {
            (
                provider.has_hooks(),
                provider.check_integration(),
                provider.display_name().to_string(),
            )
        })?;
    Some((
        facts_from_status(&status, registers_sessions_via_hooks, consent_recorded),
        display_name,
    ))
}

/// One provider's install report read into facts. Shared by the launch
/// announcement and the daemon-start reconciliation so the two can never read
/// the same report differently.
pub fn facts_from_status(
    status: &Json,
    registers_sessions_via_hooks: bool,
    consent_recorded: bool,
) -> IntegrationFacts {
    let installed = read_installed(status);
    IntegrationFacts {
        registers_sessions_via_hooks,
        state: read_state(status, installed),
        installed,
        consent_recorded,
        target: read_target(status),
        content_hash: read_content_hash(status),
        contract_current: read_contract_current(status),
    }
}

/// What the daemon-start (and plugin-reload) trigger may do about one
/// provider's integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileAuthorization {
    /// Consent is on record and the report asks for a rewrite.
    Reconcile,
    AlreadyCurrent,
    /// Nothing was ever consented to, so only the user can start this.
    ConsentMissing,
    /// The provider trusts its hooks by content hash, that trust is gone, and no
    /// consent is recorded to rewrite and re-grant it.
    TrustGrantRequired,
    /// The report says nothing this reading understands.
    Unreadable,
}

/// The gate in front of the one idempotent reconciliation. It reads the same
/// facts the launch announcement reads, so a repair can never run against
/// state the user was never told about, and never without a consent record.
pub fn authorize_reconciliation(facts: &IntegrationFacts) -> ReconcileAuthorization {
    let Some(state) = facts.state else {
        return ReconcileAuthorization::Unreadable;
    };
    if state == IntegrationState::UntrustedHash && !facts.consent_recorded {
        return ReconcileAuthorization::TrustGrantRequired;
    }
    if !facts.consent_recorded {
        return ReconcileAuthorization::ConsentMissing;
    }
    if state == IntegrationState::Installed && facts.contract_current != Some(false) {
        return ReconcileAuthorization::AlreadyCurrent;
    }
    ReconcileAuthorization::Reconcile
}

pub fn evaluate(provider_id: &str, facts: &IntegrationFacts) -> LaunchVerdict {
    if !facts.registers_sessions_via_hooks {
        return LaunchVerdict::Registering;
    }
    match facts.state {
        Some(IntegrationState::Installed) => LaunchVerdict::Registering,
        Some(
            state @ (IntegrationState::Absent
            | IntegrationState::StaleVersion
            | IntegrationState::UntrustedHash),
        ) => {
            let target = facts.target.clone();
            LaunchVerdict::Silent(IntegrationSilence {
                provider_id: provider_id.to_string(),
                level: NotifyLevel::ActionNeeded,
                remedy: match state {
                    IntegrationState::Absent if facts.consent_recorded => {
                        IntegrationRemedy::ReinstallIntegration { target }
                    }
                    IntegrationState::Absent => {
                        IntegrationRemedy::GrantConsentAndInstall { target }
                    }
                    IntegrationState::StaleVersion => {
                        IntegrationRemedy::RefreshIntegration { target }
                    }
                    IntegrationState::UntrustedHash => IntegrationRemedy::GrantHookTrust { target },
                    IntegrationState::Installed => unreachable!(),
                },
                state,
                condition: provider_integration_missing_condition(provider_id),
            })
        }
        None => LaunchVerdict::Unreadable,
    }
}

impl ServiceContext {
    /// Announce once per transition, from the tab that launched. Called on the
    /// launch thread: reading the provider's install report touches disk.
    pub(crate) fn announce_integration_silence(&self, tab_id: &str, provider_id: &str) {
        let consent_recorded = crate::config::load_config()
            .agents
            .get(provider_id)
            .and_then(|provider| {
                provider
                    .extra
                    .get(crate::config::schema::INTEGRATION_CONSENT_GRANTED)
            })
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        let Some((facts, display_name)) =
            read_provider_facts(&self.agent_registry, provider_id, consent_recorded)
        else {
            return;
        };
        match evaluate(provider_id, &facts) {
            LaunchVerdict::Registering => {
                let condition = provider_integration_missing_condition(provider_id);
                self.notif
                    .lock()
                    .observe_condition_absent(MACHINE_SCOPE, &condition);
                clear_announced(provider_id);
            }
            LaunchVerdict::Unreadable => {
                log::warn!(
                    "[integration-silence] {provider_id} declares hooks but reports no readable install state; nothing announced"
                );
            }
            LaunchVerdict::Silent(silence) => {
                let observed = AnnouncedIntegrationState::observed(silence.state, &facts);
                if !is_news(provider_id, &observed) {
                    return;
                }
                // The guard is dropped before the notification path, which
                // takes the same lock.
                let transition = self
                    .notif
                    .lock()
                    .observe_condition_present(MACHINE_SCOPE, &silence.condition);
                if transition != ConditionTransition::Announce {
                    return;
                }
                record_announced(provider_id, &observed);
                self.create_notification_ext(
                    silence.title(&display_name),
                    Some(silence.body()),
                    Some("warning"),
                    None,
                    None,
                    NotificationOrigin::local_tab(tab_id.to_string()),
                    silence.intent(),
                    None,
                );
            }
        }
    }
}
