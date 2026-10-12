use super::ServiceContext;
use codeterm_analytics::{Config, Event, Platform, ProviderId, Role, Version, Worker};
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn app_started(
    version: &str,
    platform: &str,
    mesh_machines: usize,
    workspaces: usize,
) -> Option<Event> {
    Some(Event::AppStarted {
        version: Version::from_release(version)?,
        platform: Platform::from_catalog(platform)?,
        mesh_machines: count(mesh_machines),
        workspaces: count(workspaces),
    })
}
pub(crate) fn agent_spawned(provider: Option<&str>, role: Option<&str>) -> Option<Event> {
    Some(Event::AgentSpawned {
        provider_id: ProviderId::from_catalog(provider?).unwrap_or(ProviderId::Other),
        role: role
            .map(|value| Role::from_catalog(&value.to_ascii_lowercase()).unwrap_or(Role::Other))
            .unwrap_or(Role::None),
    })
}
impl ServiceContext {
    pub(crate) fn record_product_event(&self, event: Event) {
        if let Some(worker) = self.analytics.get() {
            if worker.record_nowait(event).is_err() {
                log::debug!("[analytics] event unavailable");
            }
        }
    }
    pub(crate) fn record_product_activity(&self) {
        let mesh_machines = self
            .app_handle
            .try_state::<crate::mesh::MeshState>()
            .map_or(1, |mesh| mesh.lock().machine_count());
        let workspaces = self.mux.read().workspace_count();
        self.record_product_event(Event::ActiveDay {
            mesh_machines: count(mesh_machines),
            workspaces: count(workspaces),
        });
    }
    pub(crate) fn record_product_feature(&self, feature: codeterm_analytics::Feature) {
        self.record_product_event(Event::FeatureUsed { feature });
    }
    pub(crate) fn record_product_agent_spawned(&self, tab_id: &str) {
        let event = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| {
                agent_spawned(
                    tab.agent_type.as_deref(),
                    tab.role_profile
                        .as_deref()
                        .or_else(|| tab.role.map(|role| role.label())),
                )
            })
        };
        if let Some(event) = event {
            self.record_product_event(event);
            self.record_product_activity();
        }
    }
    pub async fn analytics_worker(&self) -> Result<&Worker, String> {
        self.analytics
            .get_or_try_init(|| async {
                let secrets = self.secrets.clone();
                let mesh_machines = self
                    .app_handle
                    .try_state::<crate::mesh::MeshState>()
                    .map_or(1, |mesh| mesh.lock().machine_count());
                let workspaces = self.mux.read().workspace_count();
                let startup = app_started(
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS,
                    mesh_machines,
                    workspaces,
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
                    if let Some(event) = startup {
                        let _ = worker.record_nowait(event);
                    }
                    let _ = worker.record_nowait(Event::ActiveDay {
                        mesh_machines: count(mesh_machines),
                        workspaces: count(workspaces),
                    });
                    if crate::update::analytics::confirm(
                        &crate::paths::data_dir().join("analytics-update.json"),
                        env!("CARGO_PKG_VERSION"),
                        &worker,
                    )
                    .is_err()
                    {
                        log::debug!("[analytics] update confirmation unavailable");
                    }
                    Ok::<_, String>(worker)
                })
                .await
                .map_err(|_| "analytics initialization failed".to_string())??;
                let observer = worker.clone();
                self.plan_store.bind_product_observer(Arc::new(move || {
                    let _ = observer.record_nowait(Event::PlanCreated {});
                }));
                if let Some(mesh) = self.app_handle.try_state::<crate::mesh::MeshState>() {
                    mesh.lock()
                        .bind_pairing_observer(mesh_observer(worker.clone()));
                }
                Ok::<_, String>(worker)
            })
            .await
    }
}
pub(crate) fn mesh_observer(worker: Worker) -> Arc<dyn Fn(usize) + Send + Sync> {
    Arc::new(move |machines| {
        let _ = worker.record_nowait(Event::MeshPeerPaired {
            mesh_machines: count(machines),
        });
    })
}
pub(crate) fn billing_observer(worker: Worker) -> codeterm_billing::ProductObserver {
    Arc::new(move |event| {
        let event = match event {
            codeterm_billing::ProductEvent::SubscriptionChanged { plan, status } => {
                subscription_event(plan, status)
            }
            codeterm_billing::ProductEvent::CheckoutStarted { plan } => Event::CheckoutStarted {
                plan: billing_plan(plan),
            },
        };
        let _ = worker.record_nowait(event);
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
