//! A provider that registers no sessions says so at launch.
//!
//! `agent_session_id` is written only by a provider's integration hook, and
//! installing that hook needs explicit consent. Without it nothing registers
//! and, until now, nothing said so — the gap surfaced hours later as a refused
//! plan write. The check here is read state: a declared capability plus the
//! install and consent records the daemon already holds. It never infers the
//! gap from an absence of sessions, which would need a window to decide in.

use serde_json::Value as Json;

use codeterm_types::notification::NotifyLevel;

use super::notification::{NotificationOrigin, PushIntent};
use super::ServiceContext;
use crate::agent::registry::AgentRegistry;
use crate::notification::dedupe::{
    provider_integration_missing_condition, ConditionTransition, MACHINE_SCOPE,
};

#[cfg(test)]
mod tests;

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
}

/// The next step the message names. Both arms are the same install path; which
/// one applies is what the consent record decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationRemedy {
    /// Nothing was ever consented to, so granting consent is the whole repair.
    GrantConsentAndInstall { target: Option<String> },
    /// Consent stands and the hooks are gone, so the same path reinstalls them.
    ReinstallIntegration { target: Option<String> },
}

impl IntegrationRemedy {
    /// Named by constant so a surface can render it and a test can assert it
    /// without either one quoting a sentence.
    pub const INSTALL_PATH: &'static str = "Settings > Integrations > Install integration";

    pub fn next_step(&self) -> &'static str {
        Self::INSTALL_PATH
    }

    pub fn target(&self) -> Option<&str> {
        match self {
            Self::GrantConsentAndInstall { target } | Self::ReinstallIntegration { target } => {
                target.as_deref()
            }
        }
    }
}

/// One launch-time announcement, before any surface renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationSilence {
    pub provider_id: String,
    pub level: NotifyLevel,
    pub remedy: IntegrationRemedy,
    /// The standing condition this announces, and the latch that keeps a
    /// second launch quiet.
    pub condition: String,
}

impl IntegrationSilence {
    pub fn title(&self, provider_display_name: &str) -> String {
        format!("{provider_display_name} registers no sessions")
    }

    /// The standing explanation, named by constant so the record and the words
    /// the user reads cannot disagree.
    pub const EXPLANATION: &'static str =
        "This provider reports its sessions through an integration that is not installed, so nothing launched here is bound to a session.";

    /// Composed from the remedy, so the message always names a next step.
    pub fn body(&self) -> String {
        let mut lines = vec![
            Self::EXPLANATION.to_string(),
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
        IntegrationFacts {
            registers_sessions_via_hooks,
            installed: read_installed(&status),
            consent_recorded,
            target: read_target(&status),
        },
        display_name,
    ))
}

pub fn evaluate(provider_id: &str, facts: &IntegrationFacts) -> LaunchVerdict {
    if !facts.registers_sessions_via_hooks {
        return LaunchVerdict::Registering;
    }
    match facts.installed {
        None => LaunchVerdict::Unreadable,
        Some(true) => LaunchVerdict::Registering,
        Some(false) => {
            let target = facts.target.clone();
            LaunchVerdict::Silent(IntegrationSilence {
                provider_id: provider_id.to_string(),
                level: NotifyLevel::ActionNeeded,
                remedy: if facts.consent_recorded {
                    IntegrationRemedy::ReinstallIntegration { target }
                } else {
                    IntegrationRemedy::GrantConsentAndInstall { target }
                },
                condition: provider_integration_missing_condition(provider_id),
            })
        }
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
            }
            LaunchVerdict::Unreadable => {
                log::warn!(
                    "[integration-silence] {provider_id} declares hooks but reports no readable install state; nothing announced"
                );
            }
            LaunchVerdict::Silent(silence) => {
                // The guard is dropped before the notification path, which
                // takes the same lock.
                let transition = self
                    .notif
                    .lock()
                    .observe_condition_present(MACHINE_SCOPE, &silence.condition);
                if transition != ConditionTransition::Announce {
                    return;
                }
                self.create_notification_ext(
                    silence.title(&display_name),
                    Some(silence.body()),
                    Some("warning"),
                    None,
                    None,
                    NotificationOrigin::local_tab(tab_id.to_string()),
                    silence.intent(),
                );
            }
        }
    }
}
