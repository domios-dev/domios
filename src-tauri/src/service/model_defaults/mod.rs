//! Keeps every stored model default pointing at a model its provider still offers.

mod inventory;
mod replacement;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use crate::agent::model_catalog;
use crate::agent::provider::ModelCatalogueAuthority;
use crate::notification::push::NotifyLevel;
use crate::service::notification::{NotificationOrigin, PushIntent};
use crate::service::ServiceState;
use inventory::{PlannedChange, StoredDefault};
use replacement::{replacement_for, CatalogueView};

#[derive(Default)]
struct Reconciler {
    listed: Mutex<HashMap<String, Vec<String>>>,
}

/// Reconciles once for every provider, then again on each catalogue change.
pub(crate) fn start(service: ServiceState) {
    let mut changes = model_catalog::subscribe_changes();
    let reconciler = Arc::new(Reconciler::default());
    tauri::async_runtime::spawn(async move {
        run_blocking(&service, &reconciler, None).await;
        loop {
            match changes.recv().await {
                Ok(change) => run_blocking(&service, &reconciler, Some(change.provider)).await,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    run_blocking(&service, &reconciler, None).await
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn run_blocking(
    service: &ServiceState,
    reconciler: &Arc<Reconciler>,
    provider: Option<String>,
) {
    let service = service.clone();
    let reconciler = reconciler.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let providers = provider
            .map(|provider| vec![provider])
            .unwrap_or_else(|| service.agent_registry.provider_ids());
        reconciler.reconcile(&service, &providers);
    })
    .await;
}

impl Reconciler {
    fn reconcile(&self, service: &ServiceState, providers: &[String]) {
        let Some(config_state) = service.app_handle.try_state::<crate::config::ConfigState>()
        else {
            return;
        };
        let config = config_state.lock().clone();
        let cron_jobs = cron_jobs(service);
        let mut stored = inventory::config_defaults(&config);
        stored.extend(inventory::cron_defaults(&cron_jobs));

        let mut changes = Vec::new();
        for provider in providers {
            changes.extend(self.plan_provider(service, provider, &stored));
        }
        if changes.is_empty() {
            return;
        }

        let mut applied = Vec::new();
        let mut next = config_state.lock().clone();
        let mut config_changed = false;
        for change in &changes {
            if inventory::apply_to_config(&mut next, change) {
                config_changed = true;
                applied.push(change.clone());
            } else if update_cron_model(service, change) {
                inventory::record(&mut next, change);
                config_changed = true;
                applied.push(change.clone());
            }
        }
        if config_changed {
            if let Err(error) =
                crate::config::save_config_and_apply(config_state.inner(), next.clone())
            {
                log::warn!("[model-defaults] could not save replaced defaults: {error}");
                return;
            }
            crate::api::events::emit_everywhere(&service.app_handle, "config-changed", &next);
        }
        notify(service, &applied);
    }

    fn plan_provider(
        &self,
        service: &ServiceState,
        provider: &str,
        stored: &[StoredDefault],
    ) -> Vec<PlannedChange> {
        let capability =
            crate::service::spawn::model_catalogue_capability(&service.agent_registry, provider);
        let live = model_catalog::resolve_live_models(provider);
        if capability.live_discovery_required && live.is_none() {
            return Vec::new();
        }
        let models = live.unwrap_or_else(|| model_catalog::resolve_models(provider));
        let recommended = model_catalog::resolve_live_default_model(provider)
            .or_else(|| model_catalog::resolve_default(provider));
        let listed: Vec<String> = models.iter().map(|model| model.id.clone()).collect();
        let previously_listed = self
            .listed
            .lock()
            .insert(provider.to_string(), listed)
            .unwrap_or_default();
        let view = CatalogueView {
            models: &models,
            recommended: recommended.as_deref(),
            authoritative: capability.authority == ModelCatalogueAuthority::Authoritative,
            previously_listed: &previously_listed,
        };
        stored
            .iter()
            .filter(|default| default.provider == provider)
            .filter_map(|default| {
                replacement_for(&default.model, &view).map(|replacement| PlannedChange {
                    stored: default.clone(),
                    replacement,
                })
            })
            .collect()
    }
}

fn cron_jobs(service: &ServiceState) -> Vec<crate::cron::store::CronJob> {
    service
        .app_handle
        .try_state::<crate::CronState>()
        .map(|cron| cron.lock().list().to_vec())
        .unwrap_or_default()
}

fn update_cron_model(service: &ServiceState, change: &PlannedChange) -> bool {
    let inventory::DefaultSlot::CronJob(id) = &change.stored.slot else {
        return false;
    };
    let (Some(cron), Some(wake)) = (
        service.app_handle.try_state::<crate::CronState>(),
        service.app_handle.try_state::<crate::CronWake>(),
    ) else {
        return false;
    };
    let patch = serde_json::from_value::<crate::cron::store::CronUpdateParams>(
        serde_json::json!({ "model": change.replacement.model }),
    );
    let Ok(patch) = patch else {
        return false;
    };
    match service.cron_update(cron.inner(), wake.inner(), id, patch) {
        Ok(_) => true,
        Err(error) => {
            log::warn!("[model-defaults] could not update cron job {id}: {error}");
            false
        }
    }
}

fn notify(service: &ServiceState, applied: &[PlannedChange]) {
    if applied.is_empty() {
        return;
    }
    let body = inventory::notice_body(applied, |provider| {
        service
            .agent_registry
            .with_provider(provider, |provider| provider.display_name().to_string())
            .unwrap_or_else(|| provider.to_string())
    });
    let title = if applied.len() == 1 {
        "A model default was updated".to_string()
    } else {
        format!("{} model defaults were updated", applied.len())
    };
    service.create_notification_ext(
        title,
        Some(body),
        Some("info"),
        None,
        None,
        NotificationOrigin::default(),
        PushIntent {
            level: Some(NotifyLevel::Verbose),
            event_key: None,
        },
        None,
    );
}
