use crate::service::ServiceState;
use codeterm_notifications::provider_hooks::spawn_reconciliation;
pub use codeterm_notifications::provider_hooks::{
    install_provider_integration_with_consent, restore_provider_integration_core,
    uninstall_provider_integration_core,
};

/// Repairs unhealthy integrations with recorded consent or an owned install.
/// Provider capability and ownership come from provider reports.
pub fn ensure_integrations_for_all(
    registry: &crate::agent::registry::AgentRegistry,
    config: &crate::config::ConfigState,
    service: ServiceState,
) {
    for provider in registry.all_provider_arcs() {
        spawn_reconciliation(
            provider,
            config.lock().agents.clone(),
            Some(config.clone()),
            Some(Box::new({
                let service = service.clone();
                move |title, body| {
                    service.log_system_notice(title, Some(body), Some("warning"));
                }
            })),
        );
    }
}

/// The same reconciliation for the one provider a plugin install or upgrade
/// just reloaded, so a machine carrying an older hook set is repaired when its
/// provider plugin updates and not only at the next daemon start.
pub fn ensure_integration_for_provider(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) {
    let consented = crate::config::load_config().agents;
    for provider in registry.all_provider_arcs() {
        if provider.id() == provider_id {
            // Startup alone infers consent for existing marked installs.
            spawn_reconciliation(provider, consented.clone(), None, None);
        }
    }
}
