use codeterm_service_core::agent;

#[cfg(test)]
mod tests;

pub(crate) fn forward_changes(
    ui: std::sync::Arc<dyn codeterm_service_core::ports::host::UiEvents>,
) {
    let mut changes = agent::model_catalog::subscribe_changes();
    codeterm_service_core::async_runtime::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(change) => ui.emit(
                    "model-catalogue-changed",
                    serde_json::to_value(change).unwrap_or_default(),
                ),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    for change in agent::model_catalog::version_snapshot() {
                        ui.emit(
                            "model-catalogue-changed",
                            serde_json::to_value(change).unwrap_or_default(),
                        );
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub(crate) fn refresh_providers(registry: &agent::registry::AgentRegistry) {
    let providers = registry.all_provider_arcs();
    log::debug!(
        "[model_catalog] refresh trigger source=startup providers={}",
        providers.len()
    );
    for provider in &providers {
        let primed = std::time::Instant::now();
        agent::model_catalog::prime_from_cache(provider.id());
        log::debug!(
            "[boot-operation] catalogue cache prime provider={} elapsed_ms={}",
            provider.id(),
            primed.elapsed().as_millis()
        );
    }
    for provider in &providers {
        agent::model_binary_watch::install_in_background(provider.clone());
    }
    codeterm_service_core::async_runtime::spawn(async move {
        for provider in providers {
            let _ = agent::model_catalog::refresh_models(provider, false).await;
        }
    });
}

pub(crate) fn reconcile_defaults(service: &codeterm_service_core::service::ServiceState) {
    codeterm_service_core::service::start_model_defaults(service.clone());
}
