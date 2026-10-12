use super::ServiceContext;
pub(crate) mod restore;
use codeterm_analytics::{
    Config, Event, ModelFamily, Platform, ProviderId, Role, TabKind, UpdateChannel, Version, Worker,
};
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn app_started(version: &str, platform: &str, channel: &str) -> Option<Event> {
    Some(Event::AppStarted {
        version: Version::from_release(version)?,
        platform: Platform::from_catalog(platform)?,
        update_channel: UpdateChannel::from_catalog(channel)?,
    })
}

pub(crate) fn tab_events(
    provider: Option<&str>,
    role: Option<&str>,
    model: Option<&str>,
) -> Vec<Event> {
    let provider_id =
        provider.map(|value| ProviderId::from_catalog(value).unwrap_or(ProviderId::Other));
    let mut events = vec![Event::TabOpened {
        kind: if provider.is_some() {
            TabKind::Agent
        } else {
            TabKind::Shell
        },
        provider_id,
    }];
    if let Some(provider_id) = provider_id {
        events.push(Event::AgentSpawned {
            provider_id,
            role: role
                .map(|value| Role::from_catalog(&value.to_ascii_lowercase()).unwrap_or(Role::Other))
                .unwrap_or(Role::None),
            model_family: model
                .and_then(|value| {
                    ModelFamily::from_catalog(value.split('[').next().unwrap_or(value))
                })
                .unwrap_or(ModelFamily::Unknown),
        });
    }
    events
}

impl ServiceContext {
    pub(crate) fn record_product_event(&self, event: Event) {
        if let Some(worker) = self.analytics.get() {
            if worker.record_nowait(event).is_err() {
                log::debug!("[analytics] event unavailable");
            }
        }
    }

    pub(crate) fn record_product_tab_created(&self, tab_id: &str) {
        let event = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            Event::TabOpened {
                kind: if tab.agent_type.is_some() {
                    TabKind::Agent
                } else {
                    TabKind::Shell
                },
                provider_id: tab
                    .agent_type
                    .as_deref()
                    .map(|value| ProviderId::from_catalog(value).unwrap_or(ProviderId::Other)),
            }
        };
        self.record_product_event(event);
    }

    pub(crate) fn record_product_restore_refused(&self, tab_id: &str) {
        let event = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| tab.boot_stage == Some(super::boot_stage::BootStage::SpawnFailed))
                .and_then(|tab| tab.agent_type.as_deref())
                .map(|provider| Event::AgentRestored {
                    provider_id: ProviderId::from_catalog(provider).unwrap_or(ProviderId::Other),
                    outcome: codeterm_analytics::RestoreOutcome::Failed,
                })
        };
        if let Some(event) = event {
            self.record_product_event(event);
        }
    }

    pub(crate) fn record_product_agent_spawned(&self, tab_id: &str, model: Option<&str>) {
        let event = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            tab_events(
                tab.agent_type.as_deref(),
                tab.role_profile
                    .as_deref()
                    .or_else(|| tab.role.map(|role| role.label())),
                model.or(tab.agent_provider_resolved_model.as_deref()),
            )
            .into_iter()
            .find(|event| matches!(event, Event::AgentSpawned { .. }))
        };
        if let Some(event) = event {
            self.record_product_event(event);
        }
    }

    pub async fn analytics_worker(&self) -> Result<&Worker, String> {
        self.analytics
            .get_or_try_init(|| async {
                let secrets = self.secrets.clone();
                let channel = self
                    .app_handle
                    .state::<crate::config::ConfigState>()
                    .lock()
                    .updater
                    .channel;
                let startup = app_started(
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS,
                    &channel.to_string(),
                );
                let worker = tokio::task::spawn_blocking(move || {
                    let reader = Arc::new(move |name: &str| {
                        tauri::async_runtime::block_on(async {
                            let store = secrets.read().await;
                            store
                                .get(name)
                                .await
                                .map(|secret| secret.value.into_inner())
                                .map_err(|_| "analytics credential unavailable".to_string())
                        })
                    });
                    let worker = Worker::start(
                        crate::paths::data_dir().join("analytics-settings.json"),
                        Config::from_env()?,
                        reader,
                    )?;
                    if crate::update::analytics::confirm(
                        &crate::paths::data_dir().join("analytics-update.json"),
                        env!("CARGO_PKG_VERSION"),
                        &worker,
                    )
                    .is_err()
                    {
                        log::debug!("[analytics] update confirmation unavailable");
                    }
                    if let Some(event) = startup {
                        let _ = worker.record_nowait(event);
                    }
                    Ok::<_, String>(worker)
                })
                .await
                .map_err(|_| "analytics initialization failed".to_string())??;
                self.plan_store
                    .bind_product_observer(plan_observer(worker.clone()));
                if let Some(mesh) = self.app_handle.try_state::<crate::mesh::MeshState>() {
                    mesh.lock()
                        .bind_pairing_observer(mesh_observer(worker.clone()));
                }
                Ok::<_, String>(worker)
            })
            .await
    }
}

pub(crate) fn plan_observer(
    worker: Worker,
) -> codeterm_plan::service_plan::product_events::ProductObserver {
    Arc::new(move |event| {
        use codeterm_plan::{
            plan::ExecutionIntent as Intent, service_plan::product_events::ProductEvent,
        };
        let event = match event {
            ProductEvent::PlanCreated => Event::PlanCreated {},
            ProductEvent::TaskDispatched {
                execution_intent: Intent::ReadOnly,
            } => Event::TaskDispatched {
                execution_intent: codeterm_analytics::ExecutionIntent::ReadOnly,
            },
            ProductEvent::TaskDispatched {
                execution_intent: Intent::Mutating,
            } => Event::TaskDispatched {
                execution_intent: codeterm_analytics::ExecutionIntent::Mutating,
            },
            ProductEvent::TaskDispatched {
                execution_intent: Intent::Unspecified,
            } => Event::TaskDispatched {
                execution_intent: codeterm_analytics::ExecutionIntent::Unspecified,
            },
        };
        let _ = worker.record_nowait(event);
    })
}
pub(crate) fn mesh_observer(worker: Worker) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        let _ = worker.record_nowait(Event::MeshPeerPaired {});
    })
}
pub(crate) fn billing_observer(worker: Worker) -> codeterm_billing::ProductObserver {
    Arc::new(move |event| {
        let event = match event {
            codeterm_billing::ProductEvent::SubscriptionChanged { plan, status } => {
                subscription_event(plan, status)
            }
            codeterm_billing::ProductEvent::CheckoutStarted { plan } => Event::CheckoutStarted {
                plan_key: billing_plan(plan),
            },
        };
        let _ = worker.record_nowait(event);
    })
}
pub(crate) fn frontend_event(
    installation: codeterm_frontend_bundle::Installation,
) -> Option<Event> {
    Some(Event::UpdateInstalled {
        from_version: Version::from_release(&installation.from_version)?,
        to_version: Version::from_release(&installation.to_version)?,
        kind: codeterm_analytics::UpdateKind::Frontend,
        attempt_count: installation.attempt_count,
    })
}
pub(crate) fn billing_plan(plan: codeterm_billing::Plan) -> codeterm_analytics::PlanKey {
    match plan {
        codeterm_billing::Plan::Free => codeterm_analytics::PlanKey::Free,
        codeterm_billing::Plan::Pro => codeterm_analytics::PlanKey::Pro,
    }
}
pub(crate) fn subscription_event(
    plan: codeterm_billing::Plan,
    status: codeterm_billing::Status,
) -> Event {
    use codeterm_analytics::SubscriptionStatus as Target;
    use codeterm_billing::Status;
    let status = match status {
        Status::Active => Target::Active,
        Status::Trialing => Target::Trialing,
        Status::PastDue => Target::PastDue,
        Status::Canceled => Target::Canceled,
        Status::Revoked => Target::Revoked,
        Status::Incomplete => Target::Incomplete,
        Status::Unpaid => Target::Unpaid,
        Status::Paused => Target::Paused,
    };
    Event::SubscriptionChanged {
        plan: billing_plan(plan),
        status,
    }
}
#[cfg(test)]
mod tests;
