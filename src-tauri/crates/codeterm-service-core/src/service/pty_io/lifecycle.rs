//! PTY lifecycle helpers: tab env, ensure/materialize, resize, kill, resolve,
//! and wait helpers.
use super::{
    restored_agent_materialization, RestoredAgentMaterialization, SEND_AGENT_BACKEND_COLS,
    SHELL_BACKEND_COLS,
};
use crate::multiplexer::commands::MuxState;
use crate::service::boot_stage::BootStage;
use crate::service::restore_admission::Admission;
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;

mod empty_session;
pub(super) mod restore_payload;

/// What a restore materialization did under its claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoredLaunchStart {
    Started { binding_nonce: String },
    NotStarted { reason: String },
}

impl RestoredLaunchStart {
    fn not_started(reason: impl Into<String>) -> Self {
        Self::NotStarted {
            reason: reason.into(),
        }
    }
}

#[derive(Clone)]
pub struct RestoredLaunch {
    pub provider_id: String,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
    pub role: Option<String>,
    pub role_profile: Option<String>,
}

pub(super) struct RestoredSnapshotLaunchInput {
    params: crate::service::SpawnAgentParams,
    launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
    pub(super) launch_task: Option<String>,
    pub(super) fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
}

pub(super) fn restored_snapshot_launch_input(
    registry: &crate::agent::registry::AgentRegistry,
    launch: RestoredLaunch,
    _historical_task: Option<String>,
    permission_bypass_authorization: Option<
        crate::service::permission_policy::PermissionBypassAuthorization,
    >,
) -> RestoredSnapshotLaunchInput {
    let input = crate::service::restored_launch::restored_fresh_launch_input(
        registry,
        crate::service::SpawnAgentParams {
            agent: Some(launch.provider_id),
            cwd: launch.cwd,
            model: launch.model,
            role: launch.role,
            role_profile: launch.role_profile,
            resume_session_id: launch.session_id,
            skip_permissions: Some(permission_bypass_authorization.is_some()),
            permission_bypass_authorization,
            ..Default::default()
        },
        launch.launch_parameters,
    );
    RestoredSnapshotLaunchInput {
        params: input.params,
        launch_parameters: input.launch_parameters,
        launch_task: input.launch_task,
        fallback_spec: None,
        origin: crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    }
}

fn restorable_launch_parameters(
    tab: &crate::multiplexer::types::Tab,
) -> crate::multiplexer::types::RestorableLaunchParameters {
    let mut parameters = tab.restorable_launch.clone();
    parameters.requested_model = parameters
        .requested_model
        .or_else(|| tab.agent_requested_model.clone());
    parameters.model = parameters
        .model
        .or_else(|| tab.agent_provider_resolved_model.clone())
        .or_else(|| tab.agent_session_model.clone());
    parameters.requested_reasoning_effort = parameters
        .requested_reasoning_effort
        .or_else(|| tab.reasoning_effort_identity.requested.clone());
    parameters.resolved_reasoning_effort = parameters
        .resolved_reasoning_effort
        .or_else(|| tab.reasoning_effort_identity.resolved.clone());
    parameters
}

fn restored_launch(tab: &crate::multiplexer::types::Tab, provider_id: String) -> RestoredLaunch {
    let launch_parameters = restorable_launch_parameters(tab);
    RestoredLaunch {
        provider_id,
        session_id: crate::service::restored_launch::restored_session_target(tab)
            .map(str::to_owned),
        cwd: tab.cwd.clone(),
        model: launch_parameters.model.clone(),
        launch_parameters,
        role: tab.role.map(|role| format!("{role:?}")),
        role_profile: tab.role_profile.clone(),
    }
}

/// A fresh launch carries a minted marker; a resume already names its session.
#[cfg(test)]
fn restored_snapshot_launch_command(
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    input: &RestoredSnapshotLaunchInput,
) -> String {
    restored_snapshot_launch_command_inner(None, registry, tab_id, provider_id, input).shell_command
}

fn restored_snapshot_launch_command_for_service(
    service: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
    input: &RestoredSnapshotLaunchInput,
) -> crate::service::orchestrator::PreparedAgentCommand {
    restored_snapshot_launch_command_inner(
        Some(service),
        &service.agent_registry,
        tab_id,
        provider_id,
        input,
    )
}

fn restored_snapshot_launch_command_inner(
    service: Option<&ServiceContext>,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    input: &RestoredSnapshotLaunchInput,
) -> crate::service::orchestrator::PreparedAgentCommand {
    if let Some(service) = service {
        return crate::service::restored_launch::RestoredFreshLaunchInput {
            params: input.params.clone(),
            launch_parameters: input.launch_parameters.clone(),
            launch_task: input.launch_task.clone(),
        }
        .prepared_command(service, tab_id)
        .unwrap_or_default();
    }
    match input.params.resume_session_id {
        Some(_) => crate::service::orchestrator::build_agent_command(
            provider_id,
            &input.params,
            input.launch_task.as_deref(),
            None,
            registry,
        ),
        None => crate::service::launch_session_detection::marked_agent_command(
            tab_id,
            provider_id,
            &input.params,
            input.launch_task.as_deref(),
            None,
            registry,
        ),
    }
    .into()
}

fn require_restored_launch_command(command: &str, session_id: Option<&str>) -> Result<(), String> {
    if !command.trim().is_empty() {
        return Ok(());
    }
    Err(match session_id {
        Some(session) => format!(
            "restored_session_unavailable: Saved session {session} exists, but the provider cannot resume it"
        ),
        None => "restored_session_unavailable: The provider cannot build a launch command".into(),
    })
}

pub fn claim_restored_pty_boot(
    mux: &MuxState,
    tab_id: &str,
) -> Result<Option<RestoredLaunch>, String> {
    let mut mux = mux.write();
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
    let Some(provider_id) = tab.agent_type.clone() else {
        return Ok(None);
    };
    if !tab.restored_from_snapshot || tab.agent_booting || tab.protected {
        return Ok(None);
    }
    tab.agent_booting = true;
    Ok(Some(restored_launch(tab, provider_id)))
}

fn matches_restore_observation(
    tab: &crate::multiplexer::types::Tab,
    provider_id: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
) -> bool {
    tab.agent_type.as_deref() == Some(provider_id)
        && crate::service::restored_launch::restored_session_target(tab) == session_id
        && tab.cwd.as_deref() == cwd
}

#[cfg(test)]
pub(crate) fn claim_restored_launch(
    mux: &MuxState,
    tab_id: &str,
    observed_provider_id: &str,
    observed_session_id: Option<&str>,
    observed_cwd: Option<&str>,
    session_valid: bool,
) -> Result<Option<RestoredLaunch>, String> {
    let mut mux = mux.write();
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
    claim_restored_tab(
        tab,
        tab_id,
        observed_provider_id,
        observed_session_id,
        observed_cwd,
        session_valid,
    )
}

fn claim_restored_tab(
    tab: &mut crate::multiplexer::types::Tab,
    tab_id: &str,
    observed_provider_id: &str,
    observed_session_id: Option<&str>,
    observed_cwd: Option<&str>,
    session_valid: bool,
) -> Result<Option<RestoredLaunch>, String> {
    let Some(provider_id) = tab.agent_type.clone() else {
        return Ok(None);
    };
    if !matches_restore_observation(tab, observed_provider_id, observed_session_id, observed_cwd) {
        return Ok(None);
    }
    if tab.restored_from_snapshot
        && tab
            .close_restore
            .as_ref()
            .is_some_and(|state| state.exact_session)
        && (!session_valid
            || crate::service::restored_launch::restored_session_target(tab).is_none())
    {
        return Err(format!(
            "The exact saved session for Tab {tab_id} is unavailable"
        ));
    }
    let Some(mode) = restored_agent_materialization(
        tab.restored_from_snapshot,
        tab.agent_booting,
        tab.protected,
        Some(&provider_id),
        crate::service::restored_launch::restored_session_target(tab),
        session_valid,
    ) else {
        return Ok(None);
    };
    let session_id = match mode {
        RestoredAgentMaterialization::Resume => {
            crate::service::restored_launch::restored_session_target(tab).map(str::to_owned)
        }
        RestoredAgentMaterialization::Fresh => None,
    };
    if matches!(mode, RestoredAgentMaterialization::Fresh) {
        tab.remember_superseded_binding_nonce();
        tab.agent_session_id = None;
    }
    // The snapshot's binding proof belongs to an exited process; voiding it makes the relaunch
    // reserve its own nonce before this claim is released.
    tab.reported_session_binding_nonce = None;
    tab.restored_from_snapshot = false;
    tab.agent_booting = true;
    let mut launch = restored_launch(tab, provider_id);
    launch.session_id = session_id;
    Ok(Some(launch))
}

#[cfg(test)]
pub(crate) fn restore_and_schedule_launch<F>(
    mux: &MuxState,
    tab_id: &str,
    provider_id: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    session_valid: bool,
    payload: Option<String>,
    schedule: F,
) -> Result<Option<RestoredLaunch>, String>
where
    F: FnOnce(RestoredLaunch, Option<String>),
{
    let Some(launch) =
        claim_restored_launch(mux, tab_id, provider_id, session_id, cwd, session_valid)?
    else {
        return Ok(None);
    };
    schedule(launch.clone(), payload);
    Ok(Some(launch))
}

/// Reattach clears stale launch failures but preserves restore failures that explain an unavailable agent.
pub(crate) fn reattach_boot_stage(
    current: Option<BootStage>,
    failure_reason: Option<&str>,
) -> Option<BootStage> {
    let restore_failure = failure_reason.is_some_and(|reason| {
        reason.starts_with("restored_provider_unresolved:")
            || reason.starts_with("restored_role_without_provider:")
            || reason.starts_with("restored_session_unavailable:")
    });
    (current == Some(BootStage::SpawnFailed) && !restore_failure)
        .then_some(BootStage::PtyReattached)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn restore_materialization_core<R, B, L>(
    mux: &MuxState,
    tab_id: &str,
    provider_id: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    session_valid: bool,
    resolve_payload: R,
    boot_stage: B,
    launcher: L,
) -> Result<Option<RestoredLaunch>, String>
where
    R: FnOnce(
        Option<&crate::multiplexer::types::PendingDeliveryRecord>,
    ) -> Result<Option<String>, String>,
    B: FnOnce(BootStage, &str, &crate::multiplexer::types::Tab),
    L: FnOnce(RestoredLaunch, Option<String>),
{
    let authority = {
        let mux = mux.read();
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
        if !tab.restored_from_snapshot
            || tab.agent_booting
            || tab.protected
            || !matches_restore_observation(tab, provider_id, session_id, cwd)
        {
            return Ok(None);
        }
        restore_payload::RestoreAuthority::capture(tab)
    };
    let payload = match resolve_payload(authority.pending.as_ref()) {
        Ok(payload) => payload,
        Err(error) => {
            if !restore_payload::commit_restore_failure(mux, tab_id, &authority, &error, |tab| {
                boot_stage(BootStage::SpawnFailed, &error, tab)
            }) {
                return Ok(None);
            }
            return Err(error);
        }
    };
    let launch = {
        let mut mux = mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return Ok(None);
        };
        if !authority.matches(tab) {
            return Ok(None);
        }
        restore_payload::retain_without_replay(tab.pending_delivery.as_mut());
        claim_restored_tab(tab, tab_id, provider_id, session_id, cwd, session_valid)?
    };
    if let Some(launch) = launch.as_ref() {
        launcher(launch.clone(), payload);
    }
    Ok(launch)
}

impl ServiceContext {
    pub fn recover_missing_restored_provider(&self, tab_id: &str) -> Result<(), String> {
        use crate::agent::provider_identity::{
            provider_for_restored_session, ProviderIdentityEvidence,
        };
        use crate::session::lifecycle::AgentTypeWriteOrigin;

        let (evidence, epoch, operation, pty_id) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {tab_id} not found"))?;
            if !tab.restored_from_snapshot
                || tab.agent_booting
                || tab.protected
                || tab.agent_type.is_some()
                || tab.agent_session_id.is_none()
            {
                return Ok(());
            }
            (
                ProviderIdentityEvidence::capture(tab),
                tab.lifecycle_epoch,
                tab.lifecycle_operation_id.clone(),
                tab.pty_id.clone(),
            )
        };
        let journal = self.lifecycle_journal.load();
        let Some(provider) = provider_for_restored_session(&journal, tab_id, &evidence) else {
            let reason = format!(
                "restored_provider_unresolved: Tab {tab_id} retains its session but has no unique lifecycle journal owner"
            );
            self.emit_boot_stage(tab_id, BootStage::SpawnFailed, Some(reason.as_str()));
            self.emit_state_changed(MuxEmitCause::TabLifecycle);
            return Err(reason);
        };
        let result = self.commit_verified_tab_agent_type_with_origin(
            tab_id,
            provider,
            AgentTypeWriteOrigin::Repair,
            None,
            &evidence,
            |mux| {
                let tab = mux.get_tab(tab_id).ok_or(())?;
                if !tab.restored_from_snapshot
                    || tab.agent_booting
                    || tab.protected
                    || tab.agent_type.is_some()
                    || tab.lifecycle_epoch != epoch
                    || tab.lifecycle_operation_id != operation
                    || tab.pty_id != pty_id
                {
                    return Err(());
                }
                Ok(())
            },
        );
        match result {
            Ok(Some(crate::service::provider_identity::ConditionalAgentTypeWrite::Committed {
                ..
            })) => {
                self.emit_state_changed(MuxEmitCause::TabLifecycle);
                Ok(())
            }
            _ => Err(format!(
                "restored_provider_changed: Tab {tab_id} changed while checking its session owner"
            )),
        }
    }

    pub(crate) fn start_backend_reader_for(&self, pty_id: String) {
        let svc_reader = std::sync::Arc::new(self.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_reader,
                    &pty_id,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => {
                        log::warn!(
                            "[ensure-pty] backend reader failed for {}: {}",
                            &pty_id[..pty_id.len().min(8)],
                            e
                        );
                    }
                }
            });
        });
    }

    pub(crate) fn restored_tab_context(&self, tab_id: &str) -> Option<String> {
        let mux = self.mux.read();
        for ws in &mux.snapshot().workspaces {
            if ws.tabs.iter().any(|id| id == tab_id) {
                return Some(ws.id.clone());
            }
        }
        None
    }

    pub fn ensure_tab_pty_for_send(&self, tab_id: &str) -> Result<(), String> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
        if let Some(refusal) = self.tab_write_lock(&resolved) {
            return Err(refusal.to_string());
        }
        if self.tab_chat_backing(&resolved).is_some() {
            return Ok(());
        }
        if let Some(refusal) = self.tab_surface_input_refusal(&resolved) {
            return Err(refusal);
        }
        crate::startup::boot_timing::mark(&resolved, None, "pty_claim_enter");
        let pty_claim = self.pty_ownership.acquire(&resolved);
        crate::startup::boot_timing::mark(&resolved, None, "pty_claim_acquired");
        self.ensure_tab_pty_for_send_inner(&pty_claim)
    }

    fn ensure_tab_pty_for_send_inner(
        &self,
        pty_claim: &crate::service::pty_ownership::TabPtyClaim,
    ) -> Result<(), String> {
        if !self.direct_restored_pty_pending(pty_claim.tab_id()) {
            self.ensure_tab_pty_linked_with_claim(pty_claim, None)?;
        }
        self.materialize_restored_agent_with_claim(pty_claim)
    }

    pub(crate) fn direct_restored_pty_pending(&self, tab_id: &str) -> bool {
        let provider_id = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| {
                    (tab.restored_from_snapshot || tab.agent_booting)
                        && tab.pty_id.is_none()
                        && !tab.protected
                })
                .and_then(|tab| tab.agent_type.clone())
        };
        provider_id
            .and_then(|id| {
                self.agent_registry
                    .with_provider(&id, crate::service::restored_launch::direct_restore_capable)
            })
            .unwrap_or(false)
    }

    pub(crate) fn ensure_tab_pty_linked_with_claim(
        &self,
        pty_claim: &crate::service::pty_ownership::TabPtyClaim,
        admission: Option<Admission>,
    ) -> Result<(), String> {
        let tab_id = pty_claim.tab_id();
        let admission = admission.unwrap_or_else(|| Admission::ungated(self, tab_id));
        if self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.close_restore
                .as_ref()
                .is_some_and(|state| state.stopped)
        }) {
            return Err("Restored shell is stopped; use Rerun command or Start shell".into());
        }
        if let Ok(pty_id) = self.resolve_pty_id(tab_id) {
            self.emit_pty_reattached(tab_id);
            admission.mark(tab_id, Some(&pty_id), "pty_admitted");
            return Ok(());
        }

        let (cwd, protected, agent_type, profile) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
            (
                tab.cwd.clone(),
                tab.protected,
                tab.agent_type.clone(),
                tab.shell_profile.clone(),
            )
        };
        if protected {
            return Err(format!(
                "Tab {} is protected and its backend PTY is not live yet",
                tab_id
            ));
        }

        crate::startup::boot_timing::mark(tab_id, None, "pty_env_enter");
        let env_vars = self.tab_env(tab_id);
        crate::startup::boot_timing::mark(tab_id, None, "pty_env_ready");
        let cols = if agent_type.is_some() {
            SEND_AGENT_BACKEND_COLS
        } else {
            SHELL_BACKEND_COLS
        };
        let t0 = std::time::Instant::now();
        crate::startup::boot_timing::mark(tab_id, None, "pty_build_enter");
        let spawn_gate = self.pty.lock().spawn_gate();
        let built = crate::pty::manager::build_shell_session_with_policy(
            spawn_gate,
            None,
            cwd.clone(),
            cols,
            40,
            env_vars,
            profile.as_ref().filter(|_| agent_type.is_none()),
            self.shell_job_policy_for_tab(tab_id),
        )?;
        crate::startup::boot_timing::mark(tab_id, None, "pty_built");
        let (pty_id, _) = self.pty.lock().register_session(None, built);
        crate::startup::boot_timing::mark(tab_id, Some(&pty_id), "pty_registered");
        super::super::pty_resize::resize(&self.pty, &self.screen_state, &pty_id, None)?;
        crate::startup::boot_timing::mark(tab_id, Some(&pty_id), "pty_resized");
        log::info!(
            "[ensure-pty] Tab {} activated backend pty {} (build={}ms)",
            tab_id,
            &pty_id[..pty_id.len().min(8)],
            t0.elapsed().as_millis()
        );
        crate::startup::boot_timing::mark(tab_id, Some(&pty_id), "pty_link_enter");
        let selection = self.link_pty_with_claim(pty_claim, tab_id, pty_id, false);
        crate::startup::boot_timing::mark(tab_id, Some(&selection.pty_id), "pty_linked");
        if !selection.linked {
            return Err(format!("Tab {} closed before PTY link", tab_id));
        }
        crate::startup::boot_timing::mark(tab_id, Some(&selection.pty_id), "pty_reader_enter");
        if let Some(reader_pty_id) = selection.reader_pty_id() {
            self.start_backend_reader_for(reader_pty_id.to_string());
        }

        admission.mark(tab_id, Some(&selection.pty_id), "pty_admitted");
        Ok(())
    }

    pub(crate) fn ensure_restored_launch_host_with_claim(
        &self,
        claim: &crate::service::pty_ownership::TabPtyClaim,
        command: &crate::service::orchestrator::PreparedAgentCommand,
        admission: Option<Admission>,
    ) -> Result<(), String> {
        if command.direct.is_none() {
            self.ensure_tab_pty_linked_with_claim(claim, admission)?;
        }
        Ok(())
    }

    fn emit_pty_reattached(&self, tab_id: &str) {
        let (current, failure_reason) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| (tab.boot_stage, tab.boot_failure_reason.clone()))
            .unwrap_or((None, None));
        if let Some(stage) = reattach_boot_stage(current, failure_reason.as_deref()) {
            self.emit_boot_stage(tab_id, stage, None);
        }
    }

    pub fn materialize_restored_agent_with_claim(
        &self,
        claim: &crate::service::pty_ownership::TabPtyClaim,
    ) -> Result<(), String> {
        self.materialize_restored_agent_with_admission(claim, None)
            .map(drop)
    }

    pub(crate) fn materialize_restored_agent_launch(
        &self,
        claim: &crate::service::pty_ownership::TabPtyClaim,
    ) -> Result<RestoredLaunchStart, String> {
        self.materialize_restored_agent_with_admission(claim, None)
    }

    pub(crate) fn materialize_restored_agent_with_admission(
        &self,
        claim: &crate::service::pty_ownership::TabPtyClaim,
        admission: Option<Admission>,
    ) -> Result<RestoredLaunchStart, String> {
        let tab_id = claim.tab_id();
        let unrestorable = self.mux.read().get_tab(tab_id).map(|tab| {
            if tab.protected {
                Some("recipient tab is protected")
            } else if !tab.restored_from_snapshot || tab.agent_booting {
                Some("recipient is no longer awaiting restore")
            } else {
                None
            }
        });
        match unrestorable {
            None => return Ok(RestoredLaunchStart::not_started("recipient tab is gone")),
            Some(Some(reason)) => return Ok(RestoredLaunchStart::not_started(reason)),
            Some(None) => {}
        }
        crate::startup::boot_timing::mark(tab_id, None, "restore_prepare_enter");
        self.recover_missing_restored_provider(tab_id)?;
        let (provider_id, mut session_id, cwd, launch_parameters, exact_session, session_cwd) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
            (
                tab.agent_type.clone(),
                crate::service::restored_launch::restored_session_target(tab).map(str::to_owned),
                tab.cwd.clone(),
                restorable_launch_parameters(tab),
                tab.restored_from_snapshot
                    && tab
                        .close_restore
                        .as_ref()
                        .is_some_and(|state| state.exact_session),
                tab.close_restore
                    .as_ref()
                    .and_then(|state| state.session_cwd.clone())
                    .or_else(|| tab.cwd.clone()),
            )
        };
        let Some(provider_id) = provider_id else {
            return Ok(RestoredLaunchStart::not_started(
                "restored recipient has no provider",
            ));
        };
        let resume_enabled = exact_session
            || crate::config::provider_config_from(&self.configuration(), &provider_id)
                .resume_sessions
                .unwrap_or(true);
        let resume_session_id = crate::service::restored_launch::restored_resume_session_id(
            resume_enabled,
            session_id.as_deref(),
        );
        let session_exists = resume_session_id
            .zip(if exact_session {
                session_cwd.as_deref()
            } else {
                cwd.as_deref()
            })
            .and_then(|(sid, dir)| {
                self.agent_registry
                    .with_provider(&provider_id, |p| p.session_exists(dir, sid))
            })
            .flatten();
        crate::startup::boot_timing::mark(tab_id, None, "session_exists_checked");
        let permission_bypass_authorization =
            crate::service::restored_launch::restore_permission_bypass_authorization(
                self,
                &self.agent_registry,
                tab_id,
                &provider_id,
                &launch_parameters,
            )?;
        crate::startup::boot_timing::mark(tab_id, None, "permission_checked");
        let session_valid = resume_session_id.is_some()
            && crate::service::restored_launch::restored_session_resumable(session_exists);
        let permissions_preserved = launch_parameters.permission_bypass_requested
            == permission_bypass_authorization.is_some();
        let restored_empty = exact_session
            && permissions_preserved
            && !session_valid
            && self.prepare_empty_session_restore(
                tab_id,
                &provider_id,
                session_id.as_deref(),
                session_exists,
            );
        if restored_empty {
            session_id = None;
        }
        if exact_session && !session_valid && !restored_empty {
            let missing = if session_exists == Some(false) {
                "session_missing: "
            } else {
                ""
            };
            let reason = format!("restored_session_unavailable: {missing}The exact saved session for Tab {tab_id} is unavailable; restore will not launch another session");
            self.refuse_restored_launch(tab_id, &reason);
            return Err(reason);
        }
        let _workspace_id = self.restored_tab_context(tab_id).ok_or_else(|| {
            format!(
                "Tab {} is a restored agent Tab but has no tab context for materialization",
                tab_id
            )
        })?;
        let mut command_error = None;
        let mut launch_nonce = None;
        self.restore_durable_user_statuses(tab_id)?;
        self.reconcile_chat_send_evidence(tab_id);
        crate::startup::boot_timing::mark(tab_id, None, "delivery_evidence_checked");
        let scheduled = restore_materialization_core(
            &self.mux,
            tab_id,
            &provider_id,
            session_id.as_deref(),
            cwd.as_deref(),
            session_valid,
            restore_payload::resolve_restored_payload,
            |stage, error, tab| {
                self.publish_boot_stage_transition(tab_id, tab, stage, Some(error));
            },
            |launch, restored_task| {
                let input = restored_snapshot_launch_input(
                    &self.agent_registry,
                    launch,
                    restored_task,
                    permission_bypass_authorization.clone(),
                );
                let provider_id = input
                    .params
                    .agent
                    .clone()
                    .expect("restored launch provider");
                crate::service::restored_launch::persist_restored_launch_identity(
                    &self.mux,
                    tab_id,
                    &provider_id,
                    &input.params,
                    &input.launch_parameters,
                );
                let launch_cmd = restored_snapshot_launch_command_for_service(
                    self,
                    tab_id,
                    &provider_id,
                    &input,
                );
                crate::startup::boot_timing::mark(tab_id, None, "launch_command_built");
                if let Err(reason) = require_restored_launch_command(
                    &launch_cmd,
                    input.params.resume_session_id.as_deref(),
                ) {
                    self.refuse_restored_launch(tab_id, &reason);
                    command_error = Some(reason);
                    return;
                }
                if let Err(reason) =
                    self.ensure_restored_launch_host_with_claim(claim, &launch_cmd, admission)
                {
                    self.refuse_restored_launch(tab_id, &reason);
                    command_error = Some(reason);
                    return;
                }
                launch_nonce = crate::service::orchestrator::launch_agent_in_background_with_env(
                    self.clone(),
                    tab_id.to_string(),
                    launch_cmd,
                    provider_id,
                    input.origin,
                    false,
                    None,
                    input.launch_task,
                    input.fallback_spec,
                    Default::default(),
                    None,
                    None,
                    input.params.resume_session_id,
                );
                crate::startup::boot_timing::mark(tab_id, None, "launch_scheduled");
            },
        );
        if scheduled.is_err() {
            super::super::boot_wait::notify_boot_changed();
            crate::service::general_agent::binding_wait::notify_binding_state_changed();
            self.emit_session_binding_state(tab_id);
            self.settle_restore_report();
        }
        if let Some(error) = command_error {
            return Err(error);
        }
        if scheduled?.is_none() {
            return Ok(RestoredLaunchStart::not_started(
                "restore authority changed before the relaunch",
            ));
        }
        if restored_empty {
            self.emit_empty_restore_notice(tab_id);
        }
        self.emit_state_changed(MuxEmitCause::TabLifecycle);
        Ok(match launch_nonce {
            Some(binding_nonce) => RestoredLaunchStart::Started { binding_nonce },
            None => RestoredLaunchStart::not_started(
                self.mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.boot_failure_reason.clone())
                    .unwrap_or_else(|| "the relaunch reserved no launch binding".into()),
            ),
        })
    }

    fn refuse_restored_launch(&self, tab_id: &str, reason: &str) {
        self.void_launch_session_proof(tab_id);
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.agent_booting = false;
        }
        self.emit_boot_stage(tab_id, BootStage::SpawnFailed, Some(reason));
        self.emit_state_changed(MuxEmitCause::TabLifecycle);
    }

    pub fn resize_tab(&self, tab_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        // A chatBackend tab has no terminal to resize — graceful no-op.
        if self.tab_chat_backing(tab_id).is_some() {
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(tab_id)?;
        super::super::pty_resize::resize(&self.pty, &self.screen_state, &pty_id, Some((cols, rows)))
    }

    /// The shell reported its foreground command finished (OSC 633 `D`) while
    /// this tab still claimed a proven-up agent: the provider process exited and
    /// the shell survived it. That is a process exit, not a heuristic and not an
    /// elapsed-time guess, so it takes the same teardown a PTY death takes —
    /// which is also what arms the existing guarded-input refusal.
    /// Identity teardown only, and deliberately blind to a booting tab. Launch
    /// readiness is settled by the caller before this, never behind this gate.
    pub(crate) fn note_provider_exit_in_surviving_shell(
        &self,
        tab_id: &str,
        proof: &crate::service::launch_readiness::LaunchExitProof,
    ) -> bool {
        let observed = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return false;
            };
            if !crate::service::launch_readiness::surviving_shell_exit_is_after_launch_ack(tab) {
                return false;
            }
            // Marks the observation on the record so every reader of
            // `exit_suspected_at` sees the same fact the teardown acts on.
            tab.exit_suspected_at = Some(std::time::Instant::now());
            true
        };
        if !observed {
            return false;
        }
        log::info!("[agent-lifecycle] provider exited to a surviving shell: tab={tab_id}");
        self.clear_tab_agent_identity_after_exit_with_proof(
            tab_id,
            "provider process exited to a surviving shell",
            proof,
        )
    }

    pub(crate) fn pty_child_is_live(&self, pty_id: &str) -> bool {
        let session = { self.pty.lock().get_session(pty_id) };
        let Some(session) = session else {
            return false;
        };
        let Ok(mut child) = session.child.lock() else {
            return false;
        };
        matches!(child.try_wait(), Ok(None))
    }

    /// Kill the PTY process for a tab without removing the tab from the mux.
    /// Different from `close_tab`, which both kills the PTY and removes the tab.
    pub fn kill_tab_pty(&self, tab_id: &str) -> Result<(), String> {
        crate::service::codex_rpc::lifecycle::stop_tab(tab_id);
        // A chatBackend tab has no PTY process — close the plugin session.
        if let Some((plugin_id, session_id)) = self.tab_chat_backing(tab_id) {
            self.chatbackend_close(&plugin_id, &session_id)?;
            self.clear_tab_agent_identity_explicitly(tab_id, "user stopped the agent");
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(tab_id)?;
        self.pty_ownership.release_pty(&pty_id);
        kill_live_tab_pty(&mut self.pty.lock(), &pty_id)?;
        self.clear_tab_agent_identity_explicitly(tab_id, "user stopped the agent");
        Ok(())
    }

    /// Resolve tab_id (or alias) → pty_id.
    pub fn resolve_pty_id(&self, tab_id: &str) -> Result<String, String> {
        let mux = self.mux.read();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
        let pty_id = mux
            .get_tab(&resolved)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
        drop(mux);
        if self.pty.lock().get_session(&pty_id).is_none() {
            return Err(format!("Tab {} PTY is not live: {}", tab_id, pty_id));
        }
        Ok(pty_id)
    }

    /// Wait for a tab to get a PTY assigned by the frontend. Polls with timeout.
    pub async fn wait_for_pty(&self, tab_id: &str, timeout_secs: u64) -> Result<String, String> {
        let mux = self.mux.clone();
        let pid = tab_id.to_string();
        tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
            loop {
                {
                    let m = mux.read();
                    if let Some(tab) = m.get_tab(&pid) {
                        if let Some(ref pty_id) = tab.pty_id {
                            return pty_id.clone();
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| "Timeout waiting for PTY".to_string())
    }

    /// Wait for output buffer to grow by at least `min_bytes`.
    pub async fn wait_for_output_growth(&self, pty_id: &str, min_bytes: usize, timeout_secs: u64) {
        let ob = self.output_buf.clone();
        let pid = pty_id.to_string();
        let _ = tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async move {
            let start_len = ob.lock().len(&pid);
            loop {
                let cur_len = ob.lock().len(&pid);
                if cur_len > start_len + min_bytes {
                    return;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        })
        .await;
    }
}

#[cfg(test)]
mod lifecycle_recovery_tests;

#[cfg(test)]
#[path = "lifecycle/restored_snapshot_launch_tests.rs"]
mod restored_snapshot_launch_tests;

#[cfg(test)]
#[path = "lifecycle/close_restore_tests.rs"]
mod close_restore_tests;

pub(crate) fn kill_live_tab_pty(
    manager: &mut crate::pty::manager::PtyManager,
    pty_id: &str,
) -> Result<(), String> {
    manager.kill(pty_id)
}

#[cfg(test)]
mod kill_tab_pty_tests {
    use std::collections::HashMap;

    use super::kill_live_tab_pty;

    #[test]
    fn service_kill_tab_pty_helper_uses_kill_semantics() {
        let mut manager = crate::pty::manager::PtyManager::new();
        let (pty_id, session) = {
            let gate = manager.spawn_gate();
            crate::pty::manager::build_session(gate, None, None, 80, 24, HashMap::new())
                .map(|built| manager.register_session(None, built))
        }
        .expect("spawn default shell");

        kill_live_tab_pty(&mut manager, &pty_id).expect("kill succeeds");

        assert!(
            manager.get_session(&pty_id).is_none(),
            "service PTY kill must remove the manager entry"
        );
        // TerminateProcess is async on Windows and the reaper thread may reap
        // concurrently; poll until the child is gone (here or in background).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match session.child.lock().expect("child lock").try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "service PTY kill must terminate/reap the child path via PtyManager::kill"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}
