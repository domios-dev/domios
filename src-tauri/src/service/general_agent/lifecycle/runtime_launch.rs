use super::*;
use crate::service::general_agent::session_contract::{LaunchOwnership, TabLaunch};
use crate::service::spawn::{AgentSpawnSpec, ChatBackendLaunchMetadata};

pub(super) fn start_main(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    config: &crate::config::ConfigState,
    provider: &str,
    permission_bypass: bool,
    resume_enabled: bool,
) {
    let target = {
        let m = mux.read();
        m.resolve_tab_id(GENERAL_ALIAS)
            .and_then(|id| m.get_tab(&id).cloned())
    };
    let Some(tab) = target else {
        log::error!("[general-agent] Runtime launch requires the General tab");
        return;
    };
    let _claim = match AutomaticLaunchClaim::acquire(mux, &tab.id) {
        Ok(claim) => claim,
        Err(error) => {
            fail_general_agent_start(svc, mux, &tab.id, &error);
            return;
        }
    };
    let same_provider = tab.agent_type.as_deref() == Some(provider);
    if same_provider && tab.auto_approve != permission_bypass {
        crate::service::agent_runtime::engine::stop(&tab.id);
    }
    let force_fresh = state::take_fresh_session_requirement();
    if same_provider && !force_fresh && tab.auto_approve == permission_bypass {
        let loaded =
            crate::service::agent_runtime::engine::for_tab(&tab.id).is_some_and(|session| {
                crate::service::agent_runtime::engine::validate_binding(svc, &session).is_ok()
                    && matches!(
                        session.state.lock().phase,
                        crate::service::agent_runtime::model::RuntimePhase::Ready
                    )
            });
        if loaded {
            svc.emit_boot_stage(&tab.id, BootStage::AgentReady, None);
            return;
        }
    }
    if !same_provider {
        crate::service::agent_runtime::engine::stop(&tab.id);
    }
    if let Some(pty_id) = tab.pty_id.as_deref() {
        if let Err(reason) = terminate_pty_before_relaunch(&svc.pty, &svc.screen_state, pty_id) {
            fail_general_agent_start(svc, mux, &tab.id, &reason);
            return;
        }
        svc.pty_ownership.release_pty(pty_id);
    }
    let plan = general_agent_session_plan(
        resume_enabled,
        force_fresh,
        tab.agent_session_id.filter(|_| same_provider),
    );
    let session_id = match plan {
        GeneralAgentSessionPlan::Restore(id) => Some(id),
        GeneralAgentSessionPlan::Discover => state::load_session_id(
            &svc.agent_registry,
            provider,
            &general_dir().to_string_lossy(),
        ),
        GeneralAgentSessionPlan::FirstRunFresh | GeneralAgentSessionPlan::ConfiguredFresh => {
            crate::service::agent_runtime::engine::stop(&tab.id);
            svc.rotate_session_binding_nonce(&tab.id);
            state::clear_session_id();
            None
        }
    };
    if tab.restored_from_snapshot && session_id.is_none() && !force_fresh && resume_enabled {
        fail_general_agent_start(
            svc,
            mux,
            &tab.id,
            "Restored General runtime thread identity missing",
        );
        return;
    }
    svc.clear_tab_restored_flag(&tab.id);
    start(
        svc,
        mux,
        config,
        TabLaunch {
            tab_id: tab.id,
            session_id,
            provider,
            permission_bypass,
            ownership: LaunchOwnership::Main,
            selection: Default::default(),
        },
    );
}

pub(super) fn start(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    config: &crate::config::ConfigState,
    target: TabLaunch<'_>,
) {
    let tab_id = target.tab_id.clone();
    if let Err(error) = launch(svc, mux, config, target) {
        fail_general_agent_start(svc, mux, &tab_id, &error);
    }
}

fn launch(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    config: &crate::config::ConfigState,
    target: TabLaunch<'_>,
) -> Result<(), String> {
    let TabLaunch {
        tab_id,
        session_id,
        provider,
        permission_bypass,
        ownership,
        mut selection,
    } = target;
    let session_id = ownership.select_session(session_id);
    if !selection.preset_args.is_empty() {
        return Err("Runtime General launch does not support preset CLI arguments".into());
    }
    let resuming = session_id.is_some();
    let cwd = general_dir().to_string_lossy().to_string();
    if resuming {
        let m = mux.read();
        let tab = m
            .get_tab(&tab_id)
            .ok_or("General runtime tab disappeared")?;
        selection.model = tab.agent_requested_model.clone();
        selection.reasoning_effort = tab.reasoning_effort_identity.requested.clone();
    }
    let resolution = crate::service::spawn::resolve_fresh_agent_model_with_notice(
        &svc.configuration(),
        &svc.agent_registry,
        provider,
        selection.model.clone(),
        session_id.as_deref(),
    );
    crate::service::spawn::validate_agent_reasoning_effort(
        &svc.agent_registry,
        provider,
        resolution.model.as_deref(),
        selection.reasoning_effort.as_deref(),
    )?;
    let prompt = instructions::render_system_prompt(
        svc.configuration().general_agent.capability_hints,
        provider,
        Some(&svc.mem),
    );
    let bootstrap = (ownership.bootstrap() && !resuming)
        .then(|| compose_startup_message(&svc.mem, &general_dir().join("bootstrap.md")))
        .flatten();
    svc.set_agent_session_id(&tab_id, session_id.clone());
    let nonce = svc.ensure_session_binding_nonce(&tab_id);
    let pad = svc.ensure_agent_pad(&tab_id)?;
    let mut env = general_agent_env_vars(&tab_id, pad.env_value(), nonce, config);
    env.extend(
        svc.model_provider_launch_env(provider)
            .map_err(|error| error.to_string())?,
    );
    {
        let mut m = mux.write();
        let tab = m
            .get_tab_mut(&tab_id)
            .ok_or("General runtime tab disappeared")?;
        super::super::identity::apply_general_agent_kind(tab);
        tab.pty_id = None;
        tab.cwd = Some(cwd.clone());
        tab.provisioned_cwd = Some(cwd.clone());
        tab.auto_approve = permission_bypass;
        crate::service::spawn::persist_model_launch_identity(tab, None, &resolution);
    }
    let params = crate::service::SpawnAgentParams {
        agent: Some(provider.into()),
        existing_tab_id: Some(tab_id.clone()),
        role: Some("Manager".into()),
        role_profile: Some(super::super::GENERAL_AGENT_KIND.into()),
        cwd: Some(cwd.clone()),
        provisioned_cwd: Some(cwd.clone()),
        model: resolution.model.clone(),
        reasoning_effort: selection.reasoning_effort.clone(),
        system_prompt: Some(prompt.clone()),
        task: bootstrap.clone(),
        resume_session_id: session_id.clone(),
        ..Default::default()
    };
    let metadata = ChatBackendLaunchMetadata {
        owner_linkage: None,
        agent: AgentSpawnSpec {
            provider: provider.into(),
            existing_tab_id: Some(tab_id.clone()),
            provisioned_cwd: Some(cwd),
            role: params.role.clone(),
            role_profile: params.role_profile.clone(),
            model: resolution.model.clone(),
            reasoning_effort: selection.reasoning_effort.clone(),
            task: bootstrap,
            extra_env: env,
            resume_session_id: session_id,
            ..Default::default()
        },
        params,
    };
    let launch_config = runtime_config(
        prompt,
        resolution.model,
        selection.reasoning_effort,
        permission_bypass,
    );
    svc.launch_chatbackend_runtime_existing(&tab_id, &metadata, launch_config)?;
    let bound_session = mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.agent_session_id.clone())
        .ok_or("General runtime did not bind its thread")?;
    ownership.persist_session_id(provider, &bound_session);
    if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
        tab.agent_booting = false;
    }
    svc.emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    svc.observe_tab_state(
        &tab_id,
        crate::service::supporter_tab_state::ObservationTrigger::SpawnReady,
        "general_agent_runtime_ready",
    );
    svc.emit_state_changed();
    Ok(())
}

fn runtime_config(
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    permission_bypass: bool,
) -> serde_json::Value {
    serde_json::json!({
        "systemPrompt": prompt,
        "model": model,
        "effort": effort,
        "mode": if permission_bypass { "full-access" } else { "auto-accept-edits" },
    })
}

#[cfg(test)]
mod tests;
