use crate::agent;

pub(crate) fn forward_changes(app: &crate::app_runtime::AppHandle) {
    let mut changes = agent::model_catalog::subscribe_changes();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(change) => {
                    codeterm_events::emit_everywhere(&app, "model-catalogue-changed", change)
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    for change in agent::model_catalog::version_snapshot() {
                        codeterm_events::emit_everywhere(&app, "model-catalogue-changed", change);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub(crate) fn refresh_providers(registry: &agent::registry::AgentRegistry) {
    let providers = registry.all_provider_arcs();
    for provider in &providers {
        agent::model_catalog::prime_from_cache(provider.id());
        agent::model_binary_watch::install(provider.clone());
    }
    tauri::async_runtime::spawn(async move {
        for provider in providers {
            let _ = agent::model_catalog::refresh_models(provider, false).await;
        }
    });
}

pub(crate) fn reconcile_defaults(service: &crate::service::ServiceState) {
    crate::service::model_defaults::start(service.clone());
}
