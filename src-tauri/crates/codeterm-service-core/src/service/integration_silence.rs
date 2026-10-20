use super::ServiceContext;
use crate::notification::dedupe::{
    provider_integration_missing_condition, ConditionTransition, MACHINE_SCOPE,
};
pub use codeterm_notifications::integration_silence::{
    announced_path, authorize_reconciliation, binding_source_label, clear_announced,
    clear_hook_binding_fallback_session, evaluate, facts_from_status, has_hook_binding_fallback,
    hook_binding_fallback_source, integration_status_with_binding_fallback, is_news,
    read_content_hash, read_contract_current, read_installed, read_integration_owned,
    read_provider_facts, read_state, read_target, record_announced, record_hook_binding_source,
    reserve_hook_binding_fallback_retry, AnnouncedIntegrationState, HookBindingFallbackRetry,
    IntegrationFacts, IntegrationRemedy, IntegrationSilence, IntegrationState, LaunchVerdict,
    ReconcileAuthorization,
};

impl ServiceContext {
    /// Announce once per transition, from the tab that launched. Called on the
    /// launch thread: reading the provider's install report touches disk.
    pub(crate) fn announce_integration_silence(&self, _tab_id: &str, provider_id: &str) {
        let consent_recorded = self
            .configuration()
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
                self.log_system_notice(
                    silence.title(&display_name),
                    Some(silence.body()),
                    Some("warning"),
                );
            }
        }
    }
}
