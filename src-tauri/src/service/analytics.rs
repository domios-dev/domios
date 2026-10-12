use super::ServiceContext;
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
                tokio::task::spawn_blocking(move || {
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
                    Ok::<_, String>(worker)
                })
                .await
                .map_err(|_| "analytics initialization failed".to_string())?
            })
            .await
    }
}

#[cfg(test)]
mod tests;
