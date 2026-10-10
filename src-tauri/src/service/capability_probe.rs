use super::ServiceContext;
use cache::{ProbeCache, Start};
use codeterm_plugin::{
    model_catalog,
    provider::{CapabilityProbe, ModelInfo},
};
use codeterm_types::agent::capability_probe::{
    InitializeCapabilities, ProbeFailure, ProbeSnapshot,
};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::OnceLock, time::Duration};
use tauri::Manager;
mod cache;
mod executor;
mod process;
mod protocol;
#[cfg(test)]
mod tests;

#[derive(Clone)]
struct Request {
    svc: ServiceContext,
    tab: String,
    provider: String,
    project: PathBuf,
}
fn cache() -> &'static Mutex<ProbeCache<Request>> {
    static CACHE: OnceLock<Mutex<ProbeCache<Request>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}
fn request(svc: &ServiceContext, tab: &str) -> Result<Request, ProbeFailure> {
    let (provider, project) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab).ok_or(ProbeFailure::ProjectUnavailable)?;
        (
            tab.agent_type.clone().ok_or(ProbeFailure::Unsupported)?,
            tab.cwd
                .clone()
                .map(PathBuf::from)
                .ok_or(ProbeFailure::ProjectUnavailable)?,
        )
    };
    let enabled = svc
        .app_handle
        .try_state::<crate::config::ConfigState>()
        .is_some_and(|config| {
            config
                .lock()
                .agents
                .get(&provider)
                .and_then(|config| config.extra.get("capability_probe_enabled"))
                .and_then(toml::Value::as_bool)
                == Some(true)
        });
    if !enabled {
        return Err(ProbeFailure::Disabled);
    }
    if svc
        .agent_registry
        .with_provider(&provider, |provider| provider.capability_probe())
        .flatten()
        != Some(CapabilityProbe::StreamJsonInitialize)
    {
        return Err(ProbeFailure::Unsupported);
    }
    Ok(Request {
        svc: svc.clone(),
        tab: tab.into(),
        provider,
        project,
    })
}
fn unavailable(reason: ProbeFailure) -> ProbeSnapshot {
    ProbeSnapshot::Unavailable { reason }
}
pub(crate) fn refresh(svc: &ServiceContext, tab: &str, force: bool) -> ProbeSnapshot {
    let request = match request(svc, tab) {
        Ok(request) => request,
        Err(reason) => return unavailable(reason),
    };
    let binary = svc
        .agent_registry
        .with_provider(&request.provider, |provider| {
            provider.cli_binary().map(str::to_string)
        })
        .flatten()
        .and_then(
            |binary| match codeterm_plugin::js_runtime::probe_bin(&binary).resolution {
                codeterm_plugin::js_runtime::BinResolution::Found(path) => Some(path),
                _ => None,
            },
        );
    let Some(binary) = binary else {
        let result = unavailable(ProbeFailure::BinaryUnavailable);
        publish(&request, &result);
        return result;
    };
    let Some(fingerprint) = model_catalog::fingerprint_path(&binary) else {
        let result = unavailable(ProbeFailure::BinaryUnavailable);
        publish(&request, &result);
        return result;
    };
    let Ok(project) = request.project.canonicalize() else {
        let result = unavailable(ProbeFailure::ProjectUnavailable);
        publish(&request, &result);
        return result;
    };
    let key = (request.provider.clone(), project);
    let start = {
        cache()
            .lock()
            .begin(key.clone(), fingerprint.clone(), request.clone(), force)
    };
    let token = match start {
        Start::Cached(snapshot) => {
            publish(&request, &snapshot);
            return snapshot;
        }
        Start::Run(token) => token,
    };
    publish(&request, &ProbeSnapshot::Pending);
    let snapshot = match executor::capture(&binary, &request.project, Duration::from_secs(10)) {
        Ok(data) => ProbeSnapshot::Ready { data },
        Err(reason) => unavailable(reason),
    };
    if !cache().lock().finish(&key, token, snapshot.clone()) {
        return unavailable(ProbeFailure::Superseded);
    }
    if let ProbeSnapshot::Ready { data } = &snapshot {
        merge_models(&request.provider, &request.project, data);
    }
    publish(&request, &snapshot);
    snapshot
}
pub(crate) fn refresh_tab(svc: &ServiceContext, tab: &str) {
    let svc = svc.clone();
    let tab = tab.to_string();
    if let Err(error) = std::thread::Builder::new()
        .name("provider-initialize-probe".into())
        .spawn(move || {
            refresh(&svc, &tab, false);
        })
    {
        log::warn!("[capability-probe] worker could not start: {error}");
    }
}
pub(crate) fn invalidate_and_rebuild(provider: &str) {
    model_catalog::clear_capability_overlay(provider);
    let requests = cache().lock().invalidate(provider);
    for request in requests {
        clear_published(&request);
        let tabs: Vec<_> = request
            .svc
            .mux
            .read()
            .all_tabs()
            .filter(|tab| {
                tab.agent_type.as_deref() == Some(provider)
                    && tab.cwd.as_deref().map(PathBuf::from).as_ref() == Some(&request.project)
            })
            .map(|tab| tab.id.clone())
            .collect();
        publish(&request, &ProbeSnapshot::Pending);
        for tab in tabs {
            refresh_tab(&request.svc, &tab);
        }
    }
}

fn publish(request: &Request, snapshot: &ProbeSnapshot) {
    let enabled = self::request(&request.svc, &request.tab).is_ok();
    if !enabled {
        return;
    }
    let mut mux = request.svc.mux.write();
    let tabs: Vec<_> = mux
        .all_tabs()
        .filter(|tab| {
            tab.agent_type.as_deref() == Some(&request.provider)
                && tab.cwd.as_deref().map(PathBuf::from).as_ref() == Some(&request.project)
        })
        .map(|tab| tab.id.clone())
        .collect();
    for id in tabs {
        if let Some(tab) = mux.get_tab_mut(&id) {
            tab.provider_signals.capabilities = Some(snapshot.clone());
        }
    }
    drop(mux);
    request.svc.emit_state_changed();
}
fn merge_models(provider: &str, project: &std::path::Path, data: &InitializeCapabilities) {
    let Some(live) = &data.models else {
        return;
    };
    let defaults = model_catalog::registered_snapshot(provider);
    let models = merge_model_entries(defaults.models, live)
        .into_iter()
        .filter(|model| live.iter().any(|item| item.id == model.id))
        .collect();
    model_catalog::put_capability_overlay(provider, &project.to_string_lossy(), models);
}
pub(super) fn merge_model_entries(
    mut models: Vec<ModelInfo>,
    live: &[codeterm_types::agent::capability_probe::ProbeModel],
) -> Vec<ModelInfo> {
    for item in live {
        if item.id.is_empty() || item.id.len() > 256 {
            continue;
        }
        if let Some(model) = models.iter_mut().find(|model| model.id == item.id) {
            model.available = Some(true);
            model.availability = Some(codeterm_plugin::provider::ModelAvailability::Available);
            if let Some(name) = &item.display_name {
                model.display_name = name.clone();
            }
            if item.description.is_some() {
                model.description = item.description.clone();
            }
        } else {
            models.push(ModelInfo {
                id: item.id.clone(),
                display_name: item.display_name.clone().unwrap_or_else(|| item.id.clone()),
                description: item.description.clone(),
                available: Some(true),
                availability: Some(codeterm_plugin::provider::ModelAvailability::Available),
                reasoning_efforts: Some(Vec::new()),
                ..Default::default()
            });
        }
    }
    models
}

fn clear_published(request: &Request) {
    let mut mux = request.svc.mux.write();
    let ids: Vec<_> = mux
        .all_tabs()
        .filter(|tab| tab.agent_type.as_deref() == Some(&request.provider))
        .map(|tab| tab.id.clone())
        .collect();
    for id in ids {
        if let Some(tab) = mux.get_tab_mut(&id) {
            tab.provider_signals.capabilities = None;
        }
    }
    drop(mux);
    request.svc.emit_state_changed();
}
