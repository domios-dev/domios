use super::{publish_available, spawn_utility, ProviderUpdateHost};
use codeterm_plugin::provider::AgentProvider;
use std::sync::Arc;

pub fn refresh_available(service: &impl ProviderUpdateHost, provider_id: &str) {
    let Some(provider) = service.provider(provider_id) else {
        publish_available(service, provider_id, None);
        return;
    };
    if !provider.supports_update_check() || provider.provider_self_update_capability().is_none() {
        publish_available(service, provider_id, None);
        return;
    }
    let service = service.clone();
    spawn_utility("provider-version-discovery", move || {
        discover(&service, provider)
    });
}

fn discover(service: &impl ProviderUpdateHost, provider: Arc<dyn AgentProvider>) {
    let available = provider.check_for_update();
    if !service
        .provider(provider.id())
        .is_some_and(|current| Arc::ptr_eq(&current, &provider))
    {
        log::debug!(
            "[provider-update] {}: discovery superseded by provider reload",
            provider.id()
        );
        return;
    }
    log::debug!(
        "[provider-update] {}: discovered availability {available:?}",
        provider.id()
    );
    publish_available(service, provider.id(), available);
}

#[cfg(test)]
mod tests;
