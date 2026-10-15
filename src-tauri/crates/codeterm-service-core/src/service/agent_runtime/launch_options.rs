use crate::multiplexer::types::RestorableRuntimeLaunchOptions;
use crate::service::{
    launch_session_detection, permission_policy, ServiceContext, SpawnAgentParams,
};
use codeterm_plugin::agent_runtime::{validate_runtime_settings, RuntimeSettings};
use std::path::PathBuf;

fn validate_host_directories(roots: Vec<PathBuf>) -> Result<Vec<String>, String> {
    let mut directories = Vec::new();
    for root in roots {
        if !root.is_absolute() || !root.is_dir() {
            return Err("Runtime writable root must be an existing absolute directory".into());
        }
        let canonical = root.canonicalize().map_err(|reason| reason.to_string())?;
        let canonical = codeterm_paths::strip_verbatim_prefix(
            canonical
                .to_str()
                .ok_or("Runtime writable root is not Unicode")?,
        );
        if !directories.contains(&canonical) {
            directories.push(canonical);
        }
    }
    validate_runtime_settings(&RuntimeSettings {
        additional_directories: directories.clone(),
        ..Default::default()
    })
    .map_err(|reason| reason.to_string())?;
    Ok(directories)
}

fn authorized_mode(requested: Option<bool>, authorized: bool) -> Result<&'static str, String> {
    match requested {
        Some(true) if authorized => Ok("full-access"),
        Some(false) => Ok("auto-accept-edits"),
        Some(true) => Err("Runtime permission bypass requires host authorization".into()),
        None => Err("Runtime launch permission choice is unresolved".into()),
    }
}

impl ServiceContext {
    pub(crate) fn prepare_agent_runtime_launch_options(
        &self,
        tab_id: &str,
        params: &SpawnAgentParams,
    ) -> Result<(), String> {
        let scope = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| Some((tab.agent_type.clone()?, tab.session_binding_nonce.clone()?)))
            .ok_or("Runtime launch binding unavailable")?;
        let result = self.prepare_agent_runtime_launch_options_inner(tab_id, params, &scope);
        if let Err(reason) = &result {
            publish_failure(self, tab_id, &scope, reason);
        }
        result
    }

    fn prepare_agent_runtime_launch_options_inner(
        &self,
        tab_id: &str,
        params: &SpawnAgentParams,
        scope: &(String, String),
    ) -> Result<(), String> {
        let (provider, nonce, cwd) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("Runtime tab unavailable")?;
            (
                tab.agent_type
                    .clone()
                    .ok_or("Runtime provider unavailable")?,
                tab.session_binding_nonce
                    .clone()
                    .ok_or("Runtime binding unavailable")?,
                tab.cwd.clone().unwrap_or_default(),
            )
        };
        if (&provider, &nonce) != (&scope.0, &scope.1) {
            return Err("Runtime launch binding changed".into());
        }
        if !super::engine::requested(self, &provider)
            || params
                .agent
                .as_deref()
                .is_some_and(|requested| requested != provider)
        {
            return Err("Runtime launch capability or provider changed".into());
        }
        let skip = params.permission_bypass_requested(&provider);
        let mode = authorized_mode(params.skip_permissions, skip)?;
        self.ensure_agent_pad(tab_id)?;
        let project =
            launch_session_detection::canonical_project_for_launch(self, tab_id, params, &cwd);
        let roots = launch_session_detection::host_writable_roots(
            &self.agent_registry,
            &provider,
            skip,
            Some(&cwd),
            project.as_deref(),
            &self.agent_pad_data_dir,
            tab_id,
        )?;
        let options = RestorableRuntimeLaunchOptions {
            provider_id: provider.clone(),
            binding_nonce: nonce.clone(),
            mode: mode.into(),
            additional_directories: validate_host_directories(roots)?,
            network_access: launch_session_detection::host_network_access(
                &self.agent_registry,
                &provider,
                skip,
                crate::agent::provider::current_platform_tag(),
            ),
        };
        let settings = runtime_settings(&options);
        validate_runtime_settings(&settings).map_err(|reason| reason.to_string())?;
        {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(tab_id).ok_or("Runtime tab unavailable")?;
            if tab.agent_type.as_deref() != Some(provider.as_str())
                || tab.session_binding_nonce.as_deref() != Some(nonce.as_str())
            {
                return Err("Runtime launch binding changed".into());
            }
            let config = tab
                .agent_runtime_config
                .get_or_insert_with(|| serde_json::json!({}));
            let config = config
                .as_object_mut()
                .ok_or("Runtime configuration must be an object")?;
            config.insert("mode".into(), serde_json::json!(options.mode));
            config.insert(
                "additionalDirectories".into(),
                serde_json::json!(options.additional_directories),
            );
            if let Some(access) = options.network_access {
                config.insert("networkAccess".into(), serde_json::json!(access));
            } else {
                config.remove("networkAccess");
            }
            tab.restorable_launch.permission_bypass_requested = skip;
            tab.restorable_launch.runtime_launch_options = Some(options);
        }
        let outcome = crate::service::session_persist::write_session_snapshot(self, &self.mux)?;
        if !matches!(outcome, crate::session::store::SaveOutcome::Saved) {
            return Err("Runtime launch options persistence refused".into());
        }
        Ok(())
    }
}

fn publish_failure(svc: &ServiceContext, tab_id: &str, scope: &(String, String), reason: &str) {
    let event = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if tab.agent_type.as_deref() != Some(scope.0.as_str())
            || tab.session_binding_nonce.as_deref() != Some(scope.1.as_str())
        {
            return;
        }
        let mut state = super::model::RuntimeState::unloaded(tab.agent_session_id.clone());
        state.restore_timing(tab.agent_runtime_state.as_ref());
        state.degrade(
            state.epoch,
            &super::model::RuntimeFailure::Refused(reason.into()),
        );
        let snapshot = serde_json::to_value(state.snapshot()).unwrap_or(serde_json::Value::Null);
        tab.agent_runtime_state = Some(snapshot.clone());
        tab.agent_booting = false;
        serde_json::json!({"tab_id":tab_id, "session_binding_nonce":tab.session_binding_nonce, "state":snapshot})
    };
    svc.notify_event("agent-runtime-state", event);
    svc.emit_state_changed();
}

fn runtime_settings(options: &RestorableRuntimeLaunchOptions) -> RuntimeSettings {
    RuntimeSettings {
        mode: Some(options.mode.clone()),
        additional_directories: options.additional_directories.clone(),
        network_access: options.network_access,
        ..Default::default()
    }
}

pub(super) fn restore(svc: &ServiceContext, tab_id: &str) -> Result<RuntimeSettings, String> {
    let (provider, nonce, requested, settings) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Runtime tab unavailable")?;
        (
            tab.agent_type
                .clone()
                .ok_or("Runtime provider unavailable")?,
            tab.session_binding_nonce
                .clone()
                .ok_or("Runtime binding unavailable")?,
            tab.restorable_launch.permission_bypass_requested,
            super::model::settings_from_config(
                tab.agent_runtime_config
                    .as_ref()
                    .unwrap_or(&serde_json::Value::Null),
            ),
        )
    };
    let requested = requested && permission_policy::configured_full_access_for(&svc)?;
    let params = SpawnAgentParams {
        agent: Some(provider.clone()),
        skip_permissions: Some(requested),
        permission_bypass_authorization: permission_policy::authorize_permission_bypass(
            &svc.agent_registry,
            &provider,
            requested,
        ),
        ..Default::default()
    };
    svc.prepare_agent_runtime_launch_options_inner(tab_id, &params, &(provider, nonce))?;
    let mux = svc.mux.read();
    let options = mux
        .get_tab(tab_id)
        .and_then(|tab| tab.restorable_launch.runtime_launch_options.as_ref())
        .ok_or("Runtime launch options unavailable")?;
    Ok(RuntimeSettings {
        model: settings.model,
        effort: settings.effort,
        ..runtime_settings(options)
    })
}

#[cfg(test)]
#[path = "launch_options/tests.rs"]
mod tests;
