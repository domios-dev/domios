use crate::service::ServiceState;
use codeterm_notifications::provider_hooks::spawn_reconciliation;
pub use codeterm_notifications::provider_hooks::{
    install_provider_integration_with_consent, restore_provider_integration_core,
};

pub(super) fn integration_consent(svc: &super::ServiceContext, provider: &str) -> Option<bool> {
    svc.ext::<crate::config::ConfigState>()
        .lock()
        .agents
        .get(provider)?
        .extra
        .get(crate::config::schema::INTEGRATION_CONSENT_GRANTED)?
        .as_bool()
}

pub fn uninstall_provider_integration_core(
    svc: &super::ServiceContext,
    provider: &str,
) -> Result<(), String> {
    if svc.agent_registry.provider_arc(provider).is_none() {
        return Err(format!("Unknown provider: {provider}"));
    }
    uninstall_integrations(svc, provider, || {
        codeterm_notifications::provider_hooks::uninstall_provider_integration_core(
            &svc.agent_registry,
            provider,
        )
    })
}

pub(super) fn uninstall_integrations(
    svc: &super::ServiceContext,
    provider: &str,
    uninstall_usual: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _integration_write = codeterm_notifications::provider_hooks::lock_integration_changes();
    super::provider_accounts::mutate_config(svc, |config| {
        config
            .agents
            .entry(provider.into())
            .or_default()
            .extra
            .insert(
                crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
                toml::Value::Boolean(false),
            );
        Ok(())
    })?;
    let usual = uninstall_usual();
    let named = super::provider_accounts::uninstall_named_integrations(svc, provider);
    match (usual, named) {
        (Err(usual), Err(named)) => Err(format!("{usual}. Named accounts: {named}")),
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

/// Repairs unhealthy integrations with recorded consent or an owned install.
/// Provider capability and ownership come from provider reports.
pub fn ensure_integrations_for_all(
    registry: &crate::agent::registry::AgentRegistry,
    config: &crate::config::ConfigState,
    service: ServiceState,
) -> Vec<(
    String,
    std::sync::Arc<codeterm_notifications::provider_hooks::IntegrationCompletion>,
)> {
    let mut completions = Vec::new();
    for provider in registry.all_provider_arcs() {
        let id = provider.id().to_string();
        if let Some(completion) = spawn_reconciliation(
            provider,
            Some(config.clone()),
            Some(Box::new({
                let service = service.clone();
                move |title, body| {
                    service.log_system_notice(title, Some(body), Some("warning"));
                }
            })),
        ) {
            completions.push((id, completion));
        }
    }
    completions
}

/// The same reconciliation for the one provider a plugin install or upgrade
/// just reloaded, so a machine carrying an older hook set is repaired when its
/// provider plugin updates and not only at the next daemon start.
pub fn ensure_integration_for_provider(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) {
    for provider in registry.all_provider_arcs() {
        if provider.id() == provider_id {
            // Startup alone infers consent for existing marked installs.
            let _ = spawn_reconciliation(provider, None, None);
        }
    }
}
