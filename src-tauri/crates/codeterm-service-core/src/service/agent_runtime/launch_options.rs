use crate::multiplexer::types::RestorableRuntimeLaunchOptions;
use crate::service::MuxEmitCause;
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

const FULL_ACCESS_MODE_KEY: &str = "fullAccessMode";

/// The runtime config names the full-access session mode only while the prepared options carry one.
fn apply_full_access_mode(
    config: &mut serde_json::Map<String, serde_json::Value>,
    options: &RestorableRuntimeLaunchOptions,
) {
    match &options.full_access_mode {
        Some(mode) => {
            config.insert(FULL_ACCESS_MODE_KEY.into(), serde_json::json!(mode));
        }
        None => {
            config.remove(FULL_ACCESS_MODE_KEY);
        }
    }
}

/// The selected launch profile's declared approve-everything session mode, for full-access launches only.
/// A fresh spawn names its profile in the params: the tab config only receives it once the launch starts.
fn full_access_mode(
    svc: &ServiceContext,
    tab_id: &str,
    provider: &str,
    params: &SpawnAgentParams,
    full_access: bool,
) -> Option<String> {
    if !full_access {
        return None;
    }
    let profile = params.launch_profile.clone().or_else(|| {
        svc.mux.read().get_tab(tab_id).and_then(|tab| {
            tab.agent_runtime_config
                .as_ref()?
                .get("launchProfile")?
                .as_str()
                .map(str::to_owned)
        })
    })?;
    super::permission_policy::declared_full_access_mode(svc, provider, &profile)
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
        self.prepare_neighbor_restore_grant(tab_id, &provider, params)?;
        let policy =
            crate::service::execution_sandbox::for_launch(self, tab_id, &provider, params, true)?;
        let granted = policy.is_none() && params.permission_bypass_requested(&provider);
        let skip =
            policy.is_none() && super::permission_policy::launch_full_access(self, tab_id, granted);
        let requested = if skip == granted {
            params.skip_permissions
        } else {
            Some(skip)
        };
        let mode = match &policy {
            Some(policy) => policy
                .runtime_mode
                .as_deref()
                .ok_or("Execution sandbox runtime mapping absent")?,
            None => authorized_mode(requested, skip)?,
        };
        let pad = self.ensure_agent_pad(tab_id)?;
        let project =
            launch_session_detection::canonical_project_for_launch(self, tab_id, params, &cwd)?;
        let roots = if let Some(policy) = &policy {
            policy.writable_roots.iter().map(PathBuf::from).collect()
        } else if self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.neighbor_authority.is_some())
        {
            vec![pad.path().to_path_buf()]
        } else {
            launch_session_detection::host_writable_roots(
                &self.agent_registry,
                &provider,
                skip,
                Some(&cwd),
                project.as_deref(),
                &self.agent_pad_data_dir,
                tab_id,
            )?
        };
        let options = RestorableRuntimeLaunchOptions {
            provider_id: provider.clone(),
            binding_nonce: nonce.clone(),
            mode: if policy.is_some()
                || self
                    .agent_registry
                    .with_provider(&provider, |provider| {
                        provider.agent_runtime().is_some_and(|runtime| {
                            runtime
                                .runtime_modes
                                .iter()
                                .any(|declared| declared.id == mode)
                        })
                    })
                    .unwrap_or(false)
            {
                mode.into()
            } else {
                String::new()
            },
            additional_directories: validate_host_directories(roots)?,
            network_access: if policy.is_some() {
                Some(false)
            } else {
                launch_session_detection::host_network_access(
                    &self.agent_registry,
                    &provider,
                    skip,
                    crate::agent::provider::current_platform_tag(),
                )
            },
            full_access_mode: full_access_mode(self, tab_id, &provider, params, skip),
        };
        let settings = runtime_settings(&options);
        validate_runtime_settings(&settings).map_err(|reason| reason.to_string())?;
        let host_approvals = super::permission_policy::host_approvals_view(self, tab_id, skip);
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
            if options.mode.is_empty() {
                config.remove("mode");
            } else {
                config.insert("mode".into(), serde_json::json!(options.mode));
            }
            config.insert(
                "additionalDirectories".into(),
                serde_json::json!(options.additional_directories),
            );
            if let Some(access) = options.network_access {
                config.insert("networkAccess".into(), serde_json::json!(access));
            } else {
                config.remove("networkAccess");
            }
            apply_full_access_mode(config, &options);
            match &tab.restorable_launch.selected_session_mode {
                Some(mode) => {
                    config.insert(
                        super::permission_policy::SESSION_MODE_KEY.into(),
                        serde_json::json!(mode),
                    );
                }
                None => {
                    config.remove(super::permission_policy::SESSION_MODE_KEY);
                }
            }
            if let Some(view) = host_approvals {
                config.insert(
                    super::permission_policy::HOST_APPROVALS_KEY.into(),
                    serde_json::json!(view),
                );
                tab.auto_approve = skip;
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
    svc.emit_state_changed(MuxEmitCause::BootStage);
}

fn runtime_settings(options: &RestorableRuntimeLaunchOptions) -> RuntimeSettings {
    RuntimeSettings {
        mode: (!options.mode.is_empty()).then(|| options.mode.clone()),
        additional_directories: options.additional_directories.clone(),
        network_access: options.network_access,
        ..Default::default()
    }
}

pub(super) fn restore(svc: &ServiceContext, tab_id: &str) -> Result<RuntimeSettings, String> {
    let (provider, nonce, requested, settings, structured_runtime_mode) = {
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
            tab.restorable_launch.structured_runtime_mode.clone(),
        )
    };
    svc.require_neighbor_restore_grant(tab_id, requested)?;
    let sandbox = svc.mux.read().get_tab(tab_id).and_then(|tab| {
        tab.restorable_launch
            .execution_sandbox
            .as_ref()
            .map(|policy| match &policy.intent {
                crate::plan::ExecutionIntent::ReadOnly => "read-only".into(),
                _ => "workspace-write".into(),
            })
    });
    let params = SpawnAgentParams {
        sandbox,
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
    let mut restored = runtime_settings(options);
    restored.service_tier = settings.service_tier;
    restored.model = settings.model;
    restored.effort = settings.effort;
    restored.mode = structured_runtime_mode.or(settings.mode).or(restored.mode);
    Ok(restored)
}

/// Restored settings plus the provider settings of the config restore just rewrote, so a live
/// context never keeps the pre-launch snapshot (a fresh spawn's full-access mode lands here).
pub(super) fn restore_ctx(
    svc: &ServiceContext,
    tab_id: &str,
) -> Result<(RuntimeSettings, serde_json::Value), String> {
    let settings = restore(svc, tab_id)?;
    let config = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_runtime_config.clone())
        .unwrap_or(serde_json::Value::Null);
    Ok((settings, super::model::public_provider_settings(&config)))
}

#[cfg(test)]
#[path = "launch_options/tests.rs"]
mod tests;
