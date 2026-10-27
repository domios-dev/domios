use crate::{
    multiplexer::types::{Tab, TabBacking},
    service::ServiceContext,
};
use codeterm_types::plan::ExecutionIntent;

fn materialize(tab: &mut Tab, runtime_declared: bool) -> bool {
    if tab.restorable_launch.execution_mode.is_some()
        || !runtime_declared
        || !tab.restored_from_snapshot
        || tab.agent_type.is_none()
        || matches!(tab.backing, TabBacking::Surface(_))
    {
        return false;
    }
    let provider = tab.agent_type.clone().expect("Checked provider");
    tab.backing = TabBacking::ChatBackend {
        plugin_id: provider,
        session_id: tab.agent_session_id.clone().unwrap_or_default(),
    };
    tab.pty_id = None;
    tab.chat_view = true;
    tab.agent_booting = false;
    true
}

pub fn tab(svc: &ServiceContext, tab_id: &str) -> Result<bool, String> {
    let (provider, pty, restored) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return Ok(false);
        };
        (
            tab.agent_type.clone(),
            tab.pty_id.clone(),
            tab.restored_from_snapshot,
        )
    };
    let Some(provider) =
        provider.filter(|provider| restored && super::engine::requested(svc, provider))
    else {
        return Ok(false);
    };
    if pty
        .as_ref()
        .is_some_and(|id| svc.pty.lock().get_session(id).is_some())
    {
        return Err("Restored runtime tab still owns a live terminal process".into());
    }
    sanitize_saved_controls(svc, tab_id, &provider)?;
    let nonce = svc.ensure_session_binding_nonce(tab_id);
    let thread = {
        let mut mux = svc.mux.write();
        let target = mux
            .get_tab_mut(tab_id)
            .ok_or("Restored runtime tab closed")?;
        if target.agent_type.as_deref() != Some(provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(nonce.as_str())
        {
            return Err("Restored runtime binding changed".into());
        }
        if !materialize(target, true) {
            return Ok(false);
        }
        target.agent_session_id.clone()
    };
    if thread.as_deref().is_some_and(|thread| !thread.is_empty()) {
        super::authority::record_spawn(svc, tab_id, &provider, &nonce)?;
    }
    Ok(true)
}

fn sanitize_saved_controls(
    svc: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
) -> Result<(), String> {
    let (model, saved_efforts, saved_modes) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return Ok(());
        };
        let config = tab.agent_runtime_config.as_ref();
        let non_empty = |value: Option<&str>| {
            value
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        };
        (
            config
                .and_then(|config| config["model"].as_str())
                .or(tab.agent_session_model.as_deref())
                .or(tab.agent_provider_resolved_model.as_deref())
                .unwrap_or_default()
                .to_owned(),
            [
                non_empty(config.and_then(|config| config["effort"].as_str())),
                non_empty(tab.reasoning_effort_identity.requested.as_deref()),
                non_empty(tab.reasoning_effort_identity.resolved.as_deref()),
                non_empty(tab.reasoning_effort_identity.actual.as_deref()),
                non_empty(tab.restorable_launch.requested_reasoning_effort.as_deref()),
                non_empty(tab.restorable_launch.resolved_reasoning_effort.as_deref()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
            [
                non_empty(config.and_then(|config| config["mode"].as_str())),
                non_empty(tab.structured_runtime_mode.as_deref()),
                non_empty(tab.restorable_launch.structured_runtime_mode.as_deref()),
                non_empty(
                    tab.restorable_launch
                        .runtime_launch_options
                        .as_ref()
                        .map(|options| options.mode.as_str()),
                ),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        )
    };
    let (supported_efforts, supported_modes) = svc
        .agent_registry
        .with_provider(provider_id, |provider| {
            let model_efforts = provider.reasoning_efforts_for_model(Some(&model));
            let efforts = if model_efforts.is_empty() {
                provider.reasoning_efforts()
            } else {
                model_efforts
            };
            let mut modes: Vec<String> = provider
                .agent_runtime()
                .map(|control| {
                    control
                        .runtime_modes
                        .into_iter()
                        .map(|mode| mode.id)
                        .collect()
                })
                .unwrap_or_default();
            if let Some(sandbox) = provider.execution_sandbox_capability() {
                for intent in [ExecutionIntent::ReadOnly, ExecutionIntent::Mutating] {
                    if let Ok(Some(mode)) = sandbox.resolve(&intent, std::env::consts::OS, true) {
                        if let Some(runtime_mode) = mode.runtime_mode {
                            if !modes.contains(&runtime_mode) {
                                modes.push(runtime_mode);
                            }
                        }
                    }
                }
            }
            (
                efforts
                    .into_iter()
                    .map(|effort| effort.id)
                    .collect::<Vec<_>>(),
                modes,
            )
        })
        .ok_or_else(|| format!("Runtime provider {provider_id} disappeared during restore"))?;
    let unsupported_efforts = saved_efforts
        .iter()
        .filter(|effort| {
            !supported_efforts
                .iter()
                .any(|supported| supported == *effort)
        })
        .cloned()
        .collect::<Vec<_>>();
    let unsupported_modes = saved_modes
        .iter()
        .filter(|mode| !supported_modes.iter().any(|supported| supported == *mode))
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported_modes.is_empty() {
        log::error!(
            "[agent-runtime-restore] tab={tab_id}: saved runtime modes {:?} are unsupported; refusing to resume",
            unsupported_modes
        );
        return Err(format!(
            "Saved runtime modes {:?} are unsupported for provider {provider_id}; refusing to resume",
            unsupported_modes
        ));
    }
    let drop_effort = !unsupported_efforts.is_empty();
    if !drop_effort {
        return Ok(());
    }
    {
        let mut mux = svc.mux.write();
        let tab = mux
            .get_tab_mut(tab_id)
            .ok_or("Restored runtime tab closed")?;
        if tab.agent_type.as_deref() != Some(provider_id) {
            return Err("Restored runtime provider changed during settings validation".into());
        }
        if drop_effort {
            if let Some(config) = tab.agent_runtime_config.as_mut() {
                if let Some(object) = config.as_object_mut() {
                    object.remove("effort");
                }
            }
            tab.reasoning_effort_identity = Default::default();
            tab.restorable_launch.requested_reasoning_effort = None;
            tab.restorable_launch.resolved_reasoning_effort = None;
            log::warn!(
                "[agent-runtime-restore] tab={tab_id}: saved effort values {:?} are unsupported for model {}; using model default",
                unsupported_efforts, model
            );
        }
    }
    Ok(())
}

pub fn all(svc: &ServiceContext) {
    let tabs = svc
        .mux
        .read()
        .snapshot()
        .tabs
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for id in tabs {
        if let Err(reason) = tab(svc, &id) {
            log::warn!("[agent-runtime-restore] tab={id}: {reason}");
        }
    }
}

#[cfg(test)]
mod tests;
