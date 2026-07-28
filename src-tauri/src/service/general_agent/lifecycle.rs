//! Start / restart / readiness-poll for the General Agent PTY+launch flow.

mod readiness;

use super::{binding_wait, general_dir, instructions, launch_lock, state, GENERAL_ALIAS};
use codeterm_types::pty::BootStage;
use readiness::{await_agent_launch_evidence, await_agent_output};
#[cfg(test)]
use readiness::{await_agent_output_with_signal, ScreenChangeSubscription};

fn deliver_then_confirm(
    deliver: impl FnOnce() -> Result<(), String>,
    confirm: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    deliver()?;
    confirm()
}

fn deliver_before_binding<T>(
    deliver: impl FnOnce() -> Result<(), String>,
    wait_for_binding: impl FnOnce() -> T,
) -> Result<T, String> {
    deliver()?;
    Ok(wait_for_binding())
}

fn deliver_confirmed_general_message(
    svc: &crate::ServiceState,
    tab_id: &str,
    pty_id: &str,
    text: &str,
) -> Result<(), String> {
    deliver_then_confirm(
        || svc.send_launch_text_optimistic(pty_id, text, true),
        || {
            svc.confirm_pending_delivery(tab_id)
                .map_err(|error| error.to_string())
        },
    )
}

fn compose_startup_message(
    fallback_system_prompt: bool,
    system_prompt: String,
    bootstrap: Option<String>,
) -> Option<String> {
    match (fallback_system_prompt, bootstrap) {
        (true, Some(bootstrap)) => Some(format!("{system_prompt}\n\n{bootstrap}")),
        (true, None) => Some(system_prompt),
        (false, Some(bootstrap)) => Some(bootstrap),
        (false, None) => None,
    }
}

/// Start the General Agent — called from start_servers on a background thread.
///
/// 1. Ensure directories + instructions file
/// 2. Find or create General workspace (protected)
/// 3. Find or create General tab (protected, aliased)
/// 4. Spawn PTY + launch or resume agent
/// 5. Start backend reader
pub fn start_general_agent(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    _app: &tauri::AppHandle,
) {
    let Some(_launch_guard) = launch_lock::LaunchGuard::acquire() else {
        log::info!("[general-agent] Start skipped; another launch owns the agent");
        return;
    };
    let general_config = config.lock().general_agent.clone();
    let provider_name = &general_config.agent_provider;

    log::info!("[general-agent] Starting (provider={})", provider_name);
    if let Err(error) =
        crate::commands::install_provider_integration_for_launch(&svc.agent_registry, provider_name)
    {
        log::error!(
            "[general-agent] Provider integration is not ready for {}: {}",
            provider_name,
            error
        );
        return;
    }

    // Regenerate AGENT.md in a background thread.
    // If mem.lock is busy, delay the snapshot pass rather than blocking PTY resume.
    {
        let mem = svc.mem.clone();
        let provider_owned = provider_name.clone();
        std::thread::spawn(move || {
            instructions::generate_instructions(&provider_owned, Some(&mem));
        });
    }

    let general_cwd = general_dir().to_string_lossy().to_string();

    // cosmetically restored — verify with screen markers and tear down the bare shell if no agent is running.
    {
        let (tab_id_opt, current_agent, current_pty_id) = {
            let m = mux.write();
            match m.resolve_tab_id(GENERAL_ALIAS) {
                Some(pid) => {
                    let tab = m.get_tab(&pid);
                    let at = tab.and_then(|p| p.agent_type.clone());
                    let pt = tab.and_then(|p| p.pty_id.clone());
                    (Some(pid), at, pt)
                }
                None => (None, None, None),
            }
        };
        if let (Some(pid), Some(current_agent)) = (tab_id_opt.clone(), current_agent.clone()) {
            if &current_agent == provider_name {
                if let Some(pty_id) = current_pty_id.clone() {
                    let alive = pty.lock().get_session(&pty_id).is_some();
                    if alive {
                        let agent_visible = await_agent_output(
                            screen_state,
                            &svc.agent_registry,
                            &pty_id,
                            std::time::Duration::from_secs(3),
                        );
                        if agent_visible {
                            let unbound = mux
                                .write()
                                .get_tab(&pid)
                                .and_then(|p| p.agent_session_id.clone())
                                .is_none();
                            if unbound {
                                if let Some(sid) = state::load_session_id(
                                    &svc.agent_registry,
                                    provider_name,
                                    &general_cwd,
                                ) {
                                    svc.set_agent_session_id(&pid, Some(sid.clone()));
                                    log::info!("[general-agent] Restored durable session {} onto reused PTY {}", sid, pty_id);
                                }
                            }
                            log::info!("[general-agent] Already running (tab={}, pty={}, provider={}) — skipping", pid, pty_id, current_agent);
                            return;
                        }
                        log::info!("[general-agent] Tab has live PTY {} but no agent output before deadline — terminating before relaunch", pty_id);
                        if let Err(reason) =
                            terminate_pty_before_relaunch(pty, screen_state, &pty_id)
                        {
                            fail_general_agent_start(svc, mux, &pid, &reason);
                            return;
                        }
                        clear_stale_general_binding(svc, mux, &pid);
                    } else {
                        // Stale pty_id pointing at a dead session — clear it so spawn proceeds cleanly.
                        log::info!(
                            "[general-agent] Clearing stale pty_id {} (session not in PtyManager)",
                            pty_id
                        );
                        screen_state.lock().remove(&pty_id);
                        clear_stale_general_binding(svc, mux, &pid);
                    }
                }
            } else {
                log::info!(
                    "[general-agent] Provider mismatch: running={}, wanted={} — restarting",
                    current_agent,
                    provider_name
                );
                if let Some(pty_id) = current_pty_id {
                    if let Err(reason) = terminate_pty_before_relaunch(pty, screen_state, &pty_id) {
                        fail_general_agent_start(svc, mux, &pid, &reason);
                        return;
                    }
                }
                clear_stale_general_binding(svc, mux, &pid);
            }
        }
    }

    // Find General tab (already created by ensure_general_workspace in init_states)
    let (tab_id, snapshot_session_id) = {
        let m = mux.write();
        let tab = m.resolve_tab_id(GENERAL_ALIAS).or_else(|| {
            m.list_workspaces()
                .iter()
                .find(|ws| ws.protected)
                .and_then(|ws| ws.tab_ids.first().cloned())
        });
        match tab {
            Some(pid) => {
                let sid = m.get_tab(&pid).and_then(|p| p.agent_session_id.clone());
                (pid, sid)
            }
            None => {
                log::error!(
                    "[general-agent] No General Tab found — ensure_general_workspace failed?"
                );
                return;
            }
        }
    };
    let force_fresh_session = state::take_fresh_session_requirement();
    let session_id = if force_fresh_session {
        svc.set_agent_session_id(&tab_id, None);
        state::clear_session_id();
        log::info!("[general-agent] First-run launch requires a fresh provider session");
        None
    } else if snapshot_session_id.is_some() {
        snapshot_session_id
    } else if let Some(stored) =
        state::load_session_id(&svc.agent_registry, provider_name, &general_cwd)
    {
        svc.set_agent_session_id(&tab_id, Some(stored.clone()));
        log::info!("[general-agent] Loaded durable session id: {}", stored);
        Some(stored)
    } else {
        let exclude = mux.write().other_session_ids(&tab_id);
        let detected = state::detect_owned_session_id(
            &svc.agent_registry,
            provider_name,
            &general_cwd,
            &exclude,
            Some(std::time::Duration::from_secs(86400 * 7)), // 7 days max age
        );
        if let Some(ref sid) = detected {
            svc.set_agent_session_id(&tab_id, Some(sid.clone()));
            state::persist_session_id(provider_name, sid);
            log::info!(
                "[general-agent] Detected GA-owned session from filesystem: {}",
                sid
            );
        }
        detected
    };

    let session_id = if let Some(ref id) = session_id {
        let valid = svc
            .agent_registry
            .with_provider(provider_name, |p| p.session_exists(&general_cwd, id))
            .unwrap_or(false);
        let owned = valid
            && state::session_id_is_owned(&svc.agent_registry, provider_name, &general_cwd, id);
        if !owned {
            log::info!("[general-agent] Session '{}' not valid or not GA-owned for provider '{}' — clearing, will detect fresh", id, provider_name);
            svc.set_agent_session_id(&tab_id, None);
            state::clear_session_id();
            let exclude = mux.write().other_session_ids(&tab_id);
            let fresh = state::detect_owned_session_id(
                &svc.agent_registry,
                provider_name,
                &general_cwd,
                &exclude,
                Some(std::time::Duration::from_secs(86400 * 7)),
            );
            if let Some(ref sid) = fresh {
                svc.set_agent_session_id(&tab_id, Some(sid.clone()));
                state::persist_session_id(provider_name, sid);
                log::info!("[general-agent] Detected fresh GA-owned session: {}", sid);
            }
            fresh
        } else {
            state::persist_session_id(provider_name, id);
            session_id
        }
    } else {
        session_id
    };

    let cols = 120u16;
    let rows = 40u16;
    let env_vars =
        general_agent_env_vars(&tab_id, svc.rotate_session_binding_nonce(&tab_id), config);

    let pty_id = {
        let mut manager = pty.lock();
        match manager.spawn_with_id(None, None, Some(general_cwd), cols, rows, env_vars) {
            Ok((id, _)) => id,
            Err(e) => {
                log::error!("[general-agent] Failed to spawn PTY: {}", e);
                fail_general_agent_start(svc, mux, &tab_id, &e);
                return;
            }
        }
    };

    let displaced = {
        let mut m = mux.write();
        let d = m.link_pty(&tab_id, pty_id.clone());
        if let Some(tab) = m.get_tab_mut(&tab_id) {
            tab.auto_approve = true;
        }
        d
    };
    crate::service::orchestrator::tab_relink_signal::notify_tab_relinked();
    if let Some(orphan) = displaced {
        svc.kill_ptys(&[orphan]);
    }
    svc.set_and_lock_tab_agent_type(&tab_id, provider_name.to_string());
    // writes go to the wrong one.
    svc.emit_state_changed();
    log::info!("[general-agent] PTY {} linked to Tab {}", pty_id, tab_id);

    // Start backend reader on a tokio runtime (start_backend_reader uses tokio::spawn, but we're on a plain std::thread — no tokio runtime here)
    log::info!("[general-agent] Starting backend reader for PTY {}", pty_id);
    {
        let svc_clone = svc.clone();
        let pty_id_clone = pty_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_clone,
                    &pty_id_clone,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => log::error!("[general-agent] Failed to start backend reader: {}", e),
                }
            });
        });
    }
    log::info!(
        "[general-agent] Backend reader started, waiting for shell prompt on PTY {}...",
        pty_id
    );

    let shell_wait_started = std::time::Instant::now();
    let ready = crate::service::pty_io::await_shell_prompt(
        screen_state,
        &pty_id,
        std::time::Duration::from_secs(60),
    );
    if !ready {
        let waited_ms = shell_wait_started.elapsed().as_millis();
        log::warn!(
            "[general-agent] Shell did not report ready for PTY {} after {}ms",
            pty_id,
            waited_ms
        );
        fail_general_agent_start(
            svc,
            mux,
            &tab_id,
            &format!("shell readiness gave up after {}ms", waited_ms),
        );
        return;
    }
    log::info!("[general-agent] Shell ready, building launch command...");

    let use_resume = session_id.is_some();
    let mut startup_message = None;
    let cmd = if use_resume {
        let sid = session_id.as_ref().unwrap();
        match svc
            .agent_registry
            .with_provider(provider_name, |p| p.build_resume_command(sid, true))
            .flatten()
        {
            Some(cmd) => cmd,
            None => {
                log::error!(
                    "[general-agent] Provider '{}' not registered or resume unsupported",
                    provider_name
                );
                return;
            }
        }
    } else {
        // For providers supporting session-id injection, pre-generate and pin it before launch.
        // This avoids guessing by mtime after start, especially when multiple tabs share a cwd.
        let injection = svc
            .agent_registry
            .with_provider(provider_name, |p| p.session_id_injection())
            .unwrap_or(crate::agent::provider::SessionIdInjection::None);
        let pre_session_id = if injection != crate::agent::provider::SessionIdInjection::None {
            let id = uuid::Uuid::new_v4().to_string();
            svc.set_agent_session_id(&tab_id, Some(id.clone()));
            state::persist_session_id(provider_name, &id);
            log::info!(
                "[general-agent] Pre-assigned session id {} for fresh launch",
                id
            );
            Some(id)
        } else {
            None
        };
        // (allocate+register+subscribe). No-op for providers that don't need it.
        let ga_cwd = svc
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|p| p.cwd.clone())
            .unwrap_or_default();
        let system_prompt = instructions::render_system_prompt(provider_name, Some(&svc.mem));
        let fallback = svc
            .agent_registry
            .with_provider(provider_name, |provider| {
                matches!(
                    crate::service::launch_delivery::mode_for_descriptor(
                        &provider.system_prompt_delivery()
                    ),
                    crate::service::launch_delivery::LaunchDeliveryMode::ConfirmedFallback
                )
            })
            .unwrap_or(true);
        let bootstrap = std::fs::read_to_string(general_dir().join("bootstrap.md")).ok();
        startup_message = compose_startup_message(fallback, system_prompt, bootstrap);
        if let Some(message) = startup_message.as_deref() {
            if let Err(error) = svc.create_pending_delivery(
                &tab_id,
                crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                message,
            ) {
                fail_general_agent_start(svc, mux, &tab_id, &error.to_string());
                return;
            }
        }
        let params = crate::agent::provider::AgentLaunchParams {
            agent_name: None,
            task: None,
            starter_prompt: Default::default(),
            args: None,
            skip_permissions: true,
            session_id: pre_session_id,
            structured_prompt_server: crate::service::structured_prompt::ensure_for_spawn(
                svc,
                provider_name,
                &tab_id,
                &ga_cwd,
            ),
        };
        match svc.agent_registry.with_provider(provider_name, |p| {
            crate::agent::stub_launch::apply(provider_name, p.build_launch_command(&params))
        }) {
            Some(cmd) => cmd,
            None => {
                log::error!(
                    "[general-agent] Provider '{}' not registered",
                    provider_name
                );
                return;
            }
        }
    };

    log::info!("[general-agent] Launching on PTY {}: {}", pty_id, cmd);
    let payload: Vec<u8> = if cfg!(windows) {
        // Bracketed paste + sacrificial space absorbs PSReadLine/ConPTY first-byte race on Windows.
        format!("\x1b[200~ {}\x1b[201~\r", cmd).into_bytes()
    } else {
        // No bracket-paste: zsh ZLE binds the handler only after the first interactive cycle, so the markers leak as literal text on launch.
        format!("{}\r", cmd).into_bytes()
    };
    if let Err(e) = pty.lock().write(&pty_id, &payload) {
        log::error!("[general-agent] Failed to write launch command: {}", e);
        fail_general_agent_start(svc, mux, &tab_id, &e.to_string());
        return;
    }

    let launched = await_agent_launch_evidence(
        screen_state,
        &svc.agent_registry,
        mux,
        &tab_id,
        &pty_id,
        std::time::Duration::from_secs(15),
    );
    if !tab_still_owns_pty(mux, &tab_id, &pty_id) {
        log::info!(
            "[general-agent] Launch on stale PTY {} for Tab {} ignored; Tab moved on",
            pty_id,
            tab_id
        );
        return;
    }
    if !launched {
        fail_general_agent_start(svc, mux, &tab_id, "agent output deadline elapsed");
        return;
    }
    svc.emit_boot_stage(&tab_id, BootStage::ProviderReady, None);
    let bound_session = match deliver_before_binding(
        || {
            if let Some(message) = startup_message {
                deliver_confirmed_general_message(svc, &tab_id, &pty_id, &message)?;
            }
            Ok(())
        },
        || {
            if use_resume {
                return mux
                    .read()
                    .get_tab(&tab_id)
                    .and_then(|tab| tab.agent_session_id.clone());
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            binding_wait::wait_for_session_binding(deadline, || {
                mux.read()
                    .get_tab(&tab_id)
                    .and_then(|tab| tab.agent_session_id.clone())
            })
        },
    ) {
        Ok(bound_session) => bound_session,
        Err(error) => {
            fail_general_agent_start(svc, mux, &tab_id, &error);
            return;
        }
    };
    let Some(bound_session) = bound_session else {
        fail_general_agent_start(
            svc,
            mux,
            &tab_id,
            "authoritative provider session binding was not reported",
        );
        return;
    };
    if !use_resume {
        state::persist_session_id(provider_name, &bound_session);
        log::info!(
            "[general-agent] Persisted hook-bound session {}",
            bound_session
        );
    }
    if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
        tab.agent_booting = false;
    }
    svc.emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    svc.emit_state_changed();
    log::info!(
        "[general-agent] Started successfully (tab={}, pty={})",
        tab_id,
        pty_id
    );
}

/// Restart the General Agent — kill current PTY and relaunch with current config.
/// Called from Tauri command when user changes provider or manually triggers restart.
pub fn restart_general_agent(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &tauri::AppHandle,
) {
    log::info!("[general-agent] Manual restart requested");

    let mut stale_pty_id = None;
    let mut general_tab_id = None;
    {
        let m = mux.write();
        if let Some(pid) = m.resolve_tab_id(GENERAL_ALIAS) {
            general_tab_id = Some(pid.clone());
            if let Some(tab) = m.get_tab(&pid) {
                if let Some(ref pty_id) = tab.pty_id {
                    stale_pty_id = Some(pty_id.clone());
                    log::info!(
                        "[general-agent] Terminating PTY {} process tree for restart",
                        pty_id
                    );
                }
            }
            drop(m);
            clear_stale_general_binding(svc, mux, &pid);
        }
    }

    if let Some(pty_id) = stale_pty_id {
        if let Err(reason) = terminate_pty_before_relaunch(pty, screen_state, &pty_id) {
            log::error!("[general-agent] Restart blocked by PTY teardown: {reason}");
            if let Some(tab_id) = general_tab_id {
                svc.emit_boot_stage(&tab_id, BootStage::SpawnFailed, Some(&reason));
                if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
                    tab.agent_booting = false;
                }
                svc.emit_state_changed();
            }
            return;
        }
    }

    // start_general_agent). Same rationale as the lifecycle::start_general_agent path: do not block restart on mem.wake_up().
    {
        let mem = svc.mem.clone();
        let provider_name = config.lock().general_agent.agent_provider.clone();
        std::thread::spawn(move || {
            instructions::generate_instructions(&provider_name, Some(&mem));
        });
    }

    start_general_agent(svc, mux, pty, screen_state, config, app);
}

pub fn restart_general_agent_tab(
    tab_id: &str,
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    if validate_general_agent_tab_retry(tab_id, mux)? {
        restart_general_agent(svc, mux, pty, screen_state, config, app);
    }
    Ok(())
}

pub fn validate_general_agent_tab_retry(
    tab_id: &str,
    mux: &crate::MuxState,
) -> Result<bool, String> {
    validate_general_agent_tab_retry_inner(tab_id, mux, false)
}

pub fn claim_general_agent_tab_retry(tab_id: &str, mux: &crate::MuxState) -> Result<bool, String> {
    validate_general_agent_tab_retry_inner(tab_id, mux, true)
}

fn validate_general_agent_tab_retry_inner(
    tab_id: &str,
    mux: &crate::MuxState,
    claim: bool,
) -> Result<bool, String> {
    let mut m = mux.write();
    let general_tab_id = m
        .resolve_tab_id(GENERAL_ALIAS)
        .ok_or_else(|| "General Agent Tab is not registered".to_string())?;
    let requested_tab_id = m
        .resolve_tab_id(tab_id)
        .unwrap_or_else(|| tab_id.to_string());
    if requested_tab_id != general_tab_id {
        return Err(format!(
            "Retry rejected: Tab {} is not the General Agent Tab {}",
            requested_tab_id, general_tab_id
        ));
    }
    let tab = m
        .get_tab(&general_tab_id)
        .ok_or_else(|| format!("General Agent Tab {} is missing", general_tab_id))?;
    if !tab.protected {
        return Err(format!(
            "Retry rejected: General Agent Tab {} is not protected",
            general_tab_id
        ));
    }
    // `agent_booting` is the claim marker — one owner, always.
    if tab.agent_booting {
        log::info!(
            "[general-agent] Retry for Tab {} ignored; launch already claimed",
            general_tab_id
        );
        return Ok(false);
    }
    // A launch stage alone only blocks while a launch actually owns the agent.
    // Without an owner it is stale state, and refusing recovery on it is what
    // left a `shell_ready` tab unrecoverable.
    if tab.boot_stage.is_some_and(is_launch_stage) {
        if launch_lock::launch_in_progress() {
            log::info!(
                "[general-agent] Retry for Tab {} ignored; launch already in progress",
                general_tab_id
            );
            return Ok(false);
        }
        log::info!(
            "[general-agent] Tab {} latched at {:?} with no live launch; treating as stale",
            general_tab_id,
            tab.boot_stage
        );
    }
    if claim {
        let _ = tab;
        if let Some(tab) = m.get_tab_mut(&general_tab_id) {
            tab.agent_booting = true;
            tab.boot_stage = Some(BootStage::PtyStarting);
            tab.boot_failure_reason = None;
        }
    }
    Ok(true)
}

fn is_launch_stage(stage: BootStage) -> bool {
    matches!(
        stage,
        BootStage::PtyStarting
            | BootStage::PtySpawned
            | BootStage::ReaderReady
            | BootStage::ShellReady
            | BootStage::AgentLaunching
            | BootStage::LaunchWritten
            | BootStage::ProviderReady
            | BootStage::PreparingRoleInstructions
            | BootStage::DeliveringInstructions
    )
}

fn tab_still_owns_pty(mux: &crate::MuxState, tab_id: &str, pty_id: &str) -> bool {
    mux.write()
        .get_tab(tab_id)
        .and_then(|p| p.pty_id.as_deref())
        == Some(pty_id)
}

fn terminate_pty_before_relaunch(
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
) -> Result<(), String> {
    let reaper = {
        let mut manager = pty.lock();
        match manager.begin_kill(pty_id) {
            Ok(reaper) => Some(reaper),
            Err(_error) if manager.get_session(pty_id).is_none() => {
                log::info!("[general-agent] PTY {} was already torn down", pty_id);
                None
            }
            Err(error) => return Err(error),
        }
    };
    if let Some(reaper) = reaper {
        reaper.wait_bounded(std::time::Duration::from_secs(5))?;
    }
    screen_state.lock().remove(pty_id);
    Ok(())
}

fn clear_stale_general_binding(svc: &crate::ServiceState, mux: &crate::MuxState, tab_id: &str) {
    svc.set_tab_agent_type(tab_id, None);
    svc.set_agent_session_id(tab_id, None);
    state::clear_session_id();
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.pty_id = None;
    }
}

fn fail_general_agent_start(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    tab_id: &str,
    reason: &str,
) {
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.agent_booting = false;
    }
    svc.emit_boot_stage(tab_id, BootStage::SpawnFailed, Some(reason));
    svc.emit_state_changed();
}

fn general_agent_env_vars(
    tab_id: &str,
    session_binding_nonce: String,
    config: &crate::config::ConfigState,
) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    env.insert("CODETERM_TAB_ID".to_string(), tab_id.to_string());
    // Per-launch binding nonce so the session-start hook report can prove it owns this tab.
    env.insert(
        "CODETERM_SESSION_BINDING_NONCE".to_string(),
        session_binding_nonce,
    );
    env.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::socket::server::socket_display_path(),
    );
    env.insert("CODETERM_ROLE".to_string(), "Orchestrator".to_string());
    // Derive URL+port from the daemon config, not a parent CodeTerm env leak —
    // an inherited prod CODETERM_API_URL (7685) 401s this instance's token.
    let (api_url, api_port) = crate::config::api_callback_url(&config.lock().api);
    env.insert("CODETERM_API_URL".to_string(), api_url);
    env.insert("CODETERM_API_PORT".to_string(), api_port);
    // Hook callbacks POST to our REST API. Without the token they'd 401 on any non-loopback bind and silently swallow the failure via `curl … || true`.
    let tok: Option<String> = config.lock().api.token.clone();
    if let Some(tok) = tok {
        if !tok.is_empty() {
            env.insert("CODETERM_API_TOKEN".to_string(), tok);
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use crate::agent::registry::AgentRegistry;
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::pty::manager::PtyManager;
    use codeterm_pty::pty::screen_state::ScreenState;
    use codeterm_types::pty::BootStage;
    use codeterm_util::lock_hold::{TimedMutex, TimedRwLock};
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn general_delivery_confirms_only_after_submit_succeeds() {
        let confirmed = std::cell::Cell::new(false);
        let result = super::deliver_then_confirm(
            || Err("submit was not confirmed".to_string()),
            || {
                confirmed.set(true);
                Ok(())
            },
        );

        assert_eq!(result.unwrap_err(), "submit was not confirmed");
        assert!(!confirmed.get());
    }

    #[test]
    fn startup_turn_is_delivered_before_session_binding_wait() {
        let steps = std::cell::RefCell::new(Vec::new());
        let binding = super::deliver_before_binding(
            || {
                steps.borrow_mut().push("delivered");
                Ok(())
            },
            || {
                steps.borrow_mut().push("binding");
                "session-id"
            },
        )
        .unwrap();

        assert_eq!(binding, "session-id");
        assert_eq!(*steps.borrow(), ["delivered", "binding"]);
    }

    #[test]
    fn fallback_provider_receives_one_composed_startup_turn() {
        let message = super::compose_startup_message(
            true,
            "system contract".into(),
            Some("bootstrap task".into()),
        )
        .unwrap();

        assert_eq!(message, "system contract\n\nbootstrap task");
    }

    #[test]
    fn provider_with_native_system_prompt_receives_only_bootstrap_turn() {
        let message = super::compose_startup_message(
            false,
            "system contract".into(),
            Some("bootstrap task".into()),
        )
        .unwrap();

        assert_eq!(message, "bootstrap task");
    }

    fn mux_with_general_tab(stage: Option<BootStage>) -> (MuxState, String) {
        let mut mux = MultiplexerState::empty();
        super::super::workspace::ensure_general_workspace(&mut mux, true);
        let tab_id = mux
            .resolve_tab_id(super::GENERAL_ALIAS)
            .expect("general Tab");
        if let Some(tab) = mux.get_tab_mut(&tab_id) {
            tab.boot_stage = stage;
        }
        (Arc::new(TimedRwLock::new("mux", mux)), tab_id)
    }

    #[test]
    fn general_agent_env_uses_configured_api_port_not_parent_env() {
        let original_port = std::env::var_os("CODETERM_API_PORT");
        let original_url = std::env::var_os("CODETERM_API_URL");
        unsafe {
            std::env::set_var("CODETERM_API_PORT", "7685");
            std::env::set_var("CODETERM_API_URL", "http://127.0.0.1:7685");
        }
        let mut config = crate::config::schema::AppConfig::default();
        config.api.port = 7686;
        config.api.token = Some("test-token".to_string());
        let config: crate::config::ConfigState = Arc::new(Mutex::new(config));

        let env = super::general_agent_env_vars("tab-1", "nonce-1".to_string(), &config);
        unsafe {
            match original_port {
                Some(p) => std::env::set_var("CODETERM_API_PORT", p),
                None => std::env::remove_var("CODETERM_API_PORT"),
            }
            match original_url {
                Some(u) => std::env::set_var("CODETERM_API_URL", u),
                None => std::env::remove_var("CODETERM_API_URL"),
            }
        }

        assert_eq!(
            env.get("CODETERM_API_PORT").map(String::as_str),
            Some("7686")
        );
        assert_eq!(
            env.get("CODETERM_API_URL").map(String::as_str),
            Some("http://127.0.0.1:7686"),
            "URL must derive from config port, not the inherited prod 7685 env"
        );
        assert_eq!(
            env.get("CODETERM_API_TOKEN").map(String::as_str),
            Some("test-token")
        );
    }

    #[test]
    fn tab_scoped_retry_rejects_non_general_tab() {
        let (mux, general_id) = mux_with_general_tab(Some(BootStage::SpawnFailed));

        let err = super::validate_general_agent_tab_retry("other-tab", &mux).unwrap_err();

        assert!(err.contains("other-tab"), "{err}");
        assert!(err.contains(&general_id), "{err}");
    }

    #[test]
    fn tab_scoped_retry_noops_while_launch_is_in_progress() {
        let _serial = super::launch_lock::TEST_SERIAL.lock();
        let (mux, general_id) = mux_with_general_tab(Some(BootStage::PtyStarting));
        let _guard = super::launch_lock::LaunchGuard::acquire().expect("launch guard");

        let should_restart = super::validate_general_agent_tab_retry(&general_id, &mux).unwrap();

        assert!(!should_restart);
    }

    /// The stuck protected `Main`: latched at a launch stage with no launch
    /// alive. Refusing recovery on the stage alone left it unrecoverable through
    /// every CLI route.
    #[test]
    fn tab_scoped_retry_recovers_a_stale_launch_stage() {
        let _serial = super::launch_lock::TEST_SERIAL.lock();
        for stage in [BootStage::ShellReady, BootStage::AgentLaunching] {
            let (mux, general_id) = mux_with_general_tab(Some(stage));

            let should_restart =
                super::validate_general_agent_tab_retry(&general_id, &mux).unwrap();

            assert!(should_restart, "stale {stage:?} must be recoverable");
        }
    }

    #[test]
    fn tab_scoped_retry_allows_failed_general_tab() {
        let (mux, general_id) = mux_with_general_tab(Some(BootStage::SpawnFailed));

        let should_restart = super::validate_general_agent_tab_retry(&general_id, &mux).unwrap();

        assert!(should_restart);
    }

    #[test]
    fn tab_scoped_retry_claim_is_single_owner() {
        let (mux, general_id) = mux_with_general_tab(Some(BootStage::SpawnFailed));

        let first = super::claim_general_agent_tab_retry(&general_id, &mux).unwrap();
        let second = super::claim_general_agent_tab_retry(&general_id, &mux).unwrap();

        assert!(first);
        assert!(!second);
        let mux = mux.read();
        let tab = mux.get_tab(&general_id).expect("general Tab");
        assert!(tab.agent_booting);
        assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
        assert!(tab.boot_failure_reason.is_none());
    }

    #[test]
    fn agent_output_readiness_wakes_on_screen_event() {
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        let registry = Arc::new(AgentRegistry::new());
        registry.register_provider(Box::new(MockProvider::new("agent-marker")));
        let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
        let signal = subscription.signal.clone();
        let (armed_tx, armed_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter_screen = screen_state.clone();
        let waiter_registry = registry.clone();
        let waiter = std::thread::spawn(move || {
            let ready = super::await_agent_output_with_signal(
                &waiter_screen,
                &waiter_registry,
                "pty-1",
                &signal,
                Duration::from_secs(1),
                || false,
                || armed_tx.send(()).unwrap(),
            );
            done_tx.send(ready).unwrap();
        });

        armed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        screen_state.lock().process("pty-1", b"agent-marker ready");

        assert!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        waiter.join().unwrap();
        drop(subscription);
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    }

    #[test]
    fn authoritative_binding_is_launch_readiness_evidence() {
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        let registry = AgentRegistry::new();
        let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");

        assert!(super::await_agent_output_with_signal(
            &screen_state,
            &registry,
            "pty-1",
            &subscription.signal,
            Duration::ZERO,
            || true,
            || panic!("authoritative binding should resolve before waiting"),
        ));
    }

    #[test]
    fn agent_output_deadline_releases_its_subscription() {
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        let registry = AgentRegistry::new();

        assert!(!super::await_agent_output(
            &screen_state,
            &registry,
            "pty-1",
            Duration::ZERO,
        ));
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    }

    #[test]
    fn agent_output_cleanup_does_not_close_paste_render_waiter() {
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        let paste_signal = screen_state.lock().arm_screen_change_signal("pty-1");
        let generation = paste_signal.generation();
        let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");

        drop(subscription);
        screen_state.lock().process("pty-1", b"paste rendered");

        assert!(paste_signal.wait_after(generation, Duration::ZERO));
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    }

    #[test]
    fn lifecycle_subscriber_drop_keeps_concurrent_waiter_live() {
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        let first = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
        let second = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
        let generation = second.signal.generation();

        drop(first);
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 1);
        screen_state
            .lock()
            .process("pty-1", b"second waiter remains live");

        assert!(second.signal.wait_after(generation, Duration::ZERO));
        drop(second);
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    }

    #[test]
    fn agent_output_teardown_closes_armed_waiter() {
        let pty = Arc::new(TimedMutex::new("pty", PtyManager::new()));
        let (_, session) = pty
            .lock()
            .spawn_with_id(
                Some("pty-1".to_string()),
                None,
                None,
                80,
                24,
                std::collections::HashMap::new(),
            )
            .expect("spawn PTY");
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        assert!(screen_state
            .lock()
            .bind_pty_generation("pty-1", session.generation));
        let registry = Arc::new(AgentRegistry::new());
        let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
        let signal = subscription.signal.clone();
        let (armed_tx, armed_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter_screen = screen_state.clone();
        let waiter_registry = registry.clone();
        let waiter = std::thread::spawn(move || {
            let ready = super::await_agent_output_with_signal(
                &waiter_screen,
                &waiter_registry,
                "pty-1",
                &signal,
                Duration::from_secs(1),
                || false,
                || armed_tx.send(()).unwrap(),
            );
            done_tx.send(ready).unwrap();
        });

        armed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(crate::service::pty_lifecycle::handle_pty_screen_death(
            &pty,
            &screen_state,
            "pty-1",
            session.generation,
        ));

        assert!(!done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        waiter.join().unwrap();
        drop(subscription);
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
        pty.lock().kill("pty-1").expect("cleanup PTY");
    }

    #[test]
    fn stale_provider_pty_allows_relaunch_and_cleans_screen_state() {
        let pty = Arc::new(TimedMutex::new("pty", PtyManager::new()));
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        screen_state.lock().process("stale-pty", b"old output");
        let signal = screen_state.lock().arm_screen_change_signal("stale-pty");

        super::terminate_pty_before_relaunch(&pty, &screen_state, "stale-pty")
            .expect("already-removed PTY must allow the configured provider to relaunch");

        assert!(screen_state.lock().get_screen_text("stale-pty").is_none());
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
        assert!(!signal.wait_after(signal.generation(), Duration::ZERO));
    }

    #[test]
    fn repeated_lifecycle_subscriptions_release_screen_state_entries() {
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));

        for _ in 0..3 {
            let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
            assert_eq!(screen_state.lock().screen_change_subscriber_count(), 1);
            drop(subscription);
            assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
        }
    }
}
