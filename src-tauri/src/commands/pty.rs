use crate::service::restored_launch::{
    restored_fresh_launch_input, restored_resume_session_id, RestoredFreshLaunchInput,
};
use crate::service::{boot_stage::BootStage, ServiceState};
use crate::{PtyState, ScreenStateHandle};

/// Shared by the Tauri command and the web mirror's POST /api/pty/spawn.
pub async fn spawn_pty_core(
    svc: &ServiceState,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    tab_id: Option<String>,
) -> Result<String, String> {
    if let Some(id) = tab_id.as_deref() {
        if crate::service::agent_runtime::requested_for_tab(svc, id) {
            crate::service::agent_runtime::restore::tab(svc, id)?;
            return Err("This agent uses Chat; terminal launch is unavailable".into());
        }
    }
    let mut pty_claim = match tab_id.clone() {
        Some(id) => Some(svc.pty_ownership.acquire_async(id).await?),
        None => None,
    };
    let state = &svc.pty;
    let mux_state = &svc.mux;
    // Protected tabs belong to the backend lifecycle, including while it is starting.
    if let Some(ref pid) = tab_id {
        if mux_state.read().get_tab(pid).is_some_and(|tab| {
            tab.close_restore
                .as_ref()
                .is_some_and(|state| state.stopped)
        }) {
            return Err("Restored shell is stopped; use Rerun command or Start shell".into());
        }
        let (is_protected, existing) = {
            let m = mux_state.read();
            let p = m.get_tab(pid);
            (
                p.map(|p| p.protected).unwrap_or(false),
                p.and_then(|p| p.pty_id.clone()),
            )
        };
        // Reuse a live PTY when possible; remounts should not re-trigger agent resume.
        if let Some(existing) = existing {
            let alive = state.lock().get_session(&existing).is_some();
            if alive {
                let restored = mux_state
                    .read()
                    .get_tab(pid)
                    .is_some_and(|tab| tab.restored_from_snapshot && !tab.protected);
                let restore_service = svc.clone();
                return reuse_live_pty_with_claim(
                    pty_claim.take().expect("Tab claim"),
                    existing,
                    restored,
                    |claim, existing| svc.link_pty_with_claim(claim, pid, existing, false),
                    move |claim| restore_service.materialize_restored_agent_with_claim(claim),
                )
                .await;
            }
        }
        if is_protected {
            drop(pty_claim.take());
            return crate::service::await_relink(|| {
                let existing = {
                    let mux = mux_state.read();
                    let Some(tab) = mux.get_tab(pid) else {
                        return Some(Err(format!("Tab {pid} closed before PTY link")));
                    };
                    tab.pty_id.clone()
                }?;
                state
                    .lock()
                    .get_session(&existing)
                    .is_some()
                    .then_some(Ok(existing))
            })
            .await;
        }
    }

    // One stamper for every launch path: a restored tab must not come back
    // identity-poorer than the same record launched by the daemon.
    let env_svc = svc.clone();
    let env_tab_id = tab_id.clone();
    let env_vars =
        tokio::task::spawn_blocking(move || env_svc.frontend_pty_env(env_tab_id.as_deref()))
            .await
            .map_err(|error| format!("spawn_pty env join error: {error}"))?;

    let profile = tab_id.as_deref().and_then(|id| {
        let mux = mux_state.read();
        mux.get_tab(id)
            .filter(|tab| tab.agent_type.is_none())
            .and_then(|tab| tab.shell_profile.clone())
    });
    let t0 = std::time::Instant::now();
    let spawn_gate = state.lock().spawn_gate();
    let built = tokio::task::spawn_blocking(move || {
        crate::pty::manager::build_session_with_profile(
            spawn_gate,
            command,
            cwd,
            cols,
            rows,
            env_vars,
            profile.as_ref(),
        )
    })
    .await
    .map_err(|e| format!("spawn_pty join error: {}", e))??;
    let build_ms = t0.elapsed().as_millis();

    let (mut pty_id, _session) = state.lock().register_session(predicted_id, built);
    crate::service::pty_resize::resize(&svc.pty, &svc.screen_state, &pty_id, None)?;
    log::info!("PTY {} spawned (build={}ms)", pty_id, build_ms);

    let reader_pty_id = if let Some(ref pid) = tab_id {
        let selected =
            svc.link_pty_with_claim(pty_claim.as_ref().expect("Tab claim"), pid, pty_id, false);
        if !selected.linked {
            return Err(format!("Tab {} closed before PTY link", pid));
        }
        let reader_pty_id = selected.reader_pty_id().map(str::to_string);
        pty_id = selected.pty_id;
        reader_pty_id
    } else {
        Some(pty_id.clone())
    };
    if let Some(reader_pty_id) = reader_pty_id {
        if let Err(e) =
            crate::service::pty_lifecycle::start_backend_reader(&svc, &reader_pty_id, false)
        {
            log::warn!(
                "[spawn_pty] failed to start backend reader for {}: {}",
                &reader_pty_id[..reader_pty_id.len().min(8)],
                e
            );
        }
    }
    // Auto-resume tab session after shell is ready.
    if let Some(ref pid) = tab_id {
        let recovery_service = svc.clone();
        let recovery_tab = pid.clone();
        let recovery = tokio::task::spawn_blocking(move || {
            recovery_service.recover_missing_restored_provider(&recovery_tab)
        })
        .await
        .map_err(|error| format!("restore provider recovery failed: {error}"))?;
        if let Err(error) = recovery {
            if error.starts_with("restored_provider_unresolved:") {
                log::warn!("[restore] {error}");
                return Ok(pty_id);
            }
            return Err(error);
        }
        let exact_restore = mux_state.read().get_tab(pid).is_some_and(|tab| {
            tab.restored_from_snapshot
                && tab
                    .close_restore
                    .as_ref()
                    .is_some_and(|state| state.exact_session)
        });
        if exact_restore {
            let restore_service = svc.clone();
            let claim = pty_claim.take().expect("Tab claim");
            materialize_restored_pty_with_claim(claim, move |claim| {
                restore_service.materialize_restored_agent_with_claim(claim)
            })
            .await?;
            return Ok(pty_id);
        }
        let Some(restore) = crate::service::pty_io::claim_restored_pty_boot(mux_state, pid)? else {
            return Ok(pty_id);
        };
        crate::service::mark_tab_agent_launch_started(mux_state, pid);
        let stored_agent_type = restore.provider_id;
        let restored_session_id = restore.session_id;
        let resume_enabled =
            crate::config::provider_config_from(&svc.configuration(), &stored_agent_type)
                .resume_sessions
                .unwrap_or(true);
        log::debug!(
            "[restore-resume-target] tab={} provider={} saved_session={:?} resume_enabled={} cwd={:?}",
            pid, stored_agent_type, restored_session_id, resume_enabled, restore.cwd
        );
        let session_id = restored_resume_session_id(resume_enabled, restored_session_id.as_deref())
            .map(str::to_owned);
        if !resume_enabled && restored_session_id.is_some() {
            svc.set_agent_session_id(pid, None);
        }
        let tab_cwd = restore.cwd;
        let tab_model = restore.model;
        let launch_parameters = restore.launch_parameters;
        let tab_role = restore.role;
        let tab_role_profile = restore.role_profile;
        let agent_type = stored_agent_type;
        let mut should_fresh_launch = true;
        let agent_type_for_fresh = agent_type.clone();
        svc.emit_boot_stage(pid, BootStage::PtySpawned, None);
        svc.set_and_lock_tab_agent_type_from(
            pid,
            agent_type.clone(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Restore,
            None,
        );
        svc.emit_state_changed();

        // Resume only restored snapshots; direct spawn uses backend start.
        if let Some(session_id) = session_id {
            let provider_id = agent_type;

            // Clear stale id if session is gone to avoid resume loop.
            let presence = svc
                .agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.session_exists(tab_cwd.as_deref().unwrap_or_default(), &session_id)
                })
                .flatten();
            let session_valid =
                crate::service::restored_launch::restored_session_resumable(presence);
            log::debug!(
                "[restore-session-probe] tab={} provider={} session={} cwd={:?} presence={:?}",
                pid,
                provider_id,
                session_id,
                tab_cwd,
                presence
            );
            if !session_valid {
                log::info!(
                    "[resume] session {} for Tab {} is confirmed absent under {:?} — clearing, will try fresh launch",
                    session_id, pid, tab_cwd
                );
                svc.set_agent_session_id(pid, None);
                svc.emit_state_changed();
            } else {
                // Build the resume command from the persisted launch inputs.
                let permission_bypass_authorization =
                    crate::service::restored_launch::restore_permission_bypass_authorization(
                        &svc.app_handle,
                        &svc.agent_registry,
                        pid,
                        &provider_id,
                        &launch_parameters,
                    )?;
                let skip_perms = permission_bypass_authorization.is_some();
                // Reuse the restored-launch seam so identity, model, and effort arguments cannot diverge between launch paths.
                let input = restored_fresh_launch_input(
                    &svc.agent_registry,
                    crate::service::SpawnAgentParams {
                        agent: Some(provider_id.clone()),
                        cwd: tab_cwd.clone(),
                        resume_session_id: Some(session_id.clone()),
                        skip_permissions: Some(skip_perms),
                        permission_bypass_authorization,
                        model: launch_parameters.model.clone().or(tab_model.clone()),
                        role: tab_role.clone(),
                        role_profile: tab_role_profile.clone(),
                        ..Default::default()
                    },
                    launch_parameters.clone(),
                );
                let resume_cmd = input.command(svc.as_ref(), pid);
                if let Some(cmd) = resume_cmd {
                    dispatch_restored_snapshot_launch(svc.as_ref(), pid, &provider_id, &input, cmd);
                    should_fresh_launch = false;
                } else {
                    log::warn!(
                        "[resume] provider {} has no resume command for Tab {}; starting fresh",
                        provider_id,
                        pid
                    );
                    svc.set_agent_session_id(pid, None);
                    svc.emit_state_changed();
                }
            }
        }
        if should_fresh_launch {
            let provider_id = agent_type_for_fresh;
            let permission_bypass_authorization =
                crate::service::restored_launch::restore_permission_bypass_authorization(
                    &svc.app_handle,
                    &svc.agent_registry,
                    pid,
                    &provider_id,
                    &launch_parameters,
                )?;
            let skip_perms = permission_bypass_authorization.is_some();
            let input = restored_fresh_launch_input(
                &svc.agent_registry,
                crate::service::SpawnAgentParams {
                    agent: Some(provider_id.clone()),
                    cwd: tab_cwd.clone(),
                    skip_permissions: Some(skip_perms),
                    permission_bypass_authorization,
                    model: launch_parameters.model.clone().or(tab_model.clone()),
                    role: tab_role.clone(),
                    role_profile: tab_role_profile.clone(),
                    ..Default::default()
                },
                launch_parameters.clone(),
            );
            let launch_cmd = input.command(svc.as_ref(), pid);
            if let Some(cmd) = launch_cmd {
                dispatch_restored_snapshot_launch(svc.as_ref(), pid, &provider_id, &input, cmd);
            } else {
                log::error!(
                    "[restore-fresh] provider {} has no launch command for Tab {}",
                    provider_id,
                    pid
                );
                if let Some(tab) = svc.mux.write().get_tab_mut(pid) {
                    tab.agent_booting = false;
                }
                svc.emit_boot_stage(
                    pid,
                    BootStage::SpawnFailed,
                    Some("provider did not produce a restored launch command"),
                );
            }
        }
    }

    Ok(pty_id)
}

async fn reuse_live_pty_with_claim(
    claim: crate::service::pty_ownership::TabPtyClaim,
    existing: String,
    restored: bool,
    link: impl FnOnce(
        &crate::service::pty_ownership::TabPtyClaim,
        String,
    ) -> crate::service::pty_ownership::PtySelection,
    materialize: impl FnOnce(&crate::service::pty_ownership::TabPtyClaim) -> Result<(), String>
        + Send
        + 'static,
) -> Result<String, String> {
    let tab_id = claim.tab_id().to_string();
    let selected = link(&claim, existing);
    if !selected.linked {
        return Err(format!("Tab {tab_id} closed before PTY link"));
    }
    log::info!(
        "[spawn_pty] Tab {tab_id} already has live pty {} — reusing",
        selected.pty_id,
    );
    if restored {
        if let Err(error) = materialize_restored_pty_with_claim(claim, materialize).await {
            log::error!(
                "[spawn_pty] restored launch materialization failed for Tab {tab_id}: {error}"
            );
        }
    }
    Ok(selected.pty_id)
}

async fn materialize_restored_pty_with_claim(
    claim: crate::service::pty_ownership::TabPtyClaim,
    materialize: impl FnOnce(&crate::service::pty_ownership::TabPtyClaim) -> Result<(), String>
        + Send
        + 'static,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || materialize(&claim))
        .await
        .map_err(|error| format!("restore materialization task failed: {error}"))?
}

fn dispatch_restored_snapshot_launch(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    provider_id: &str,
    input: &RestoredFreshLaunchInput,
    launch_cmd: String,
) {
    crate::service::restored_launch::persist_restored_launch_identity(
        &svc.mux,
        tab_id,
        provider_id,
        &input.params,
        &input.launch_parameters,
    );
    crate::service::orchestrator::launch_agent_in_background_with_env(
        svc.clone(),
        tab_id.to_string(),
        launch_cmd,
        provider_id.to_string(),
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
        false,
        None,
        input.launch_task.clone(),
        None,
        Default::default(),
        None,
        None,
        input.params.resume_session_id.clone(),
    );
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/{id}/write.
pub fn write_pty_string_core(
    svc: &ServiceState,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    svc.require_managed_terminal_input_allowed(&pty_id, data.as_bytes())?;
    let session = {
        let manager = svc.pty.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };
    let bytes = data.into_bytes();
    parse_pty_input(svc, &svc.mux, &pty_id, &bytes);
    mark_user_activity(svc, &svc.mux, &pty_id);
    crate::pty::capture::CaptureRegistry::global().tap_input(
        &pty_id,
        "write_pty_string_core",
        &bytes,
    );
    mark_manual_agent_interrupt(svc, &pty_id, &bytes);
    let encoded = svc.encode_provider_control_key(&pty_id, &bytes);
    session
        .write_queue
        .send(encoded.map(Vec::from).unwrap_or(bytes))
}

fn mark_manual_agent_interrupt(svc: &ServiceState, pty_id: &str, bytes: &[u8]) {
    if bytes == b"\x1b" {
        svc.mark_agent_interrupted(pty_id);
    }
}

fn mark_user_activity(
    svc: &ServiceState,
    mux: &crate::multiplexer::commands::MuxState,
    pty_id: &str,
) {
    svc.user_last_activity.mark();
    let tab_id = mux.read().tab_id_for_pty(pty_id);
    if let Some(pid) = tab_id {
        svc.usage_activity.mark(&pid);
    }
}

/// Feed bytes to the input parser when the destination tab is not locked.
/// A match may claim a bare shell, but cannot contradict a claimed session.
fn parse_pty_input(
    svc: &ServiceState,
    mux: &crate::multiplexer::commands::MuxState,
    pty_id: &str,
    bytes: &[u8],
) {
    let (tab_id, locked) = {
        let m = mux.read();
        let pid = m.tab_id_for_pty(pty_id);
        let locked = pid
            .as_ref()
            .and_then(|id| m.get_tab(id))
            .map(|p| p.agent_type_locked)
            .unwrap_or(false);
        (pid, locked)
    };
    let Some(tab_id) = tab_id else { return };
    if locked {
        svc.input_parser.clear(&tab_id);
        return;
    }
    if let Some(provider_id) = svc.input_parser.process(&tab_id, bytes) {
        log::info!(
            "[input-parser] detected '{}' launch in Tab {} — locking agent_type",
            provider_id,
            tab_id,
        );
        svc.set_and_lock_tab_agent_type(&tab_id, provider_id);
        svc.emit_state_changed();
    }
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/{id}/resize.
pub fn resize_pty_core(
    state: &PtyState,
    screen_state: &ScreenStateHandle,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    crate::service::pty_resize::resize(state, screen_state, &pty_id, Some((cols, rows)))
}

#[cfg(test)]
#[path = "pty/tests.rs"]
mod tests;
