//! Start / restart / readiness-poll for the General Agent PTY+launch flow.

use super::{general_dir, instructions, state, GENERAL_ALIAS};
use codeterm_types::pty::BootStage;

/// Start the General Agent — called from start_servers on a background thread.
///
/// 1. Ensure directories + instructions file
/// 2. Find or create General workspace (protected)
/// 3. Find or create General pane (protected, aliased)
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
    let general_config = config.lock().general_agent.clone();
    let provider_name = &general_config.agent_provider;

    log::info!("[general-agent] Starting (provider={})", provider_name);

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
        let (pane_id_opt, current_agent, current_pty_id) = {
            let m = mux.write();
            match m.resolve_pane_id(GENERAL_ALIAS) {
                Some(pid) => {
                    let pane = m.get_pane(&pid);
                    let at = pane.and_then(|p| p.agent_type.clone());
                    let pt = pane.and_then(|p| p.pty_id.clone());
                    (Some(pid), at, pt)
                }
                None => (None, None, None),
            }
        };
        if let (Some(pid), Some(current_agent)) = (pane_id_opt.clone(), current_agent.clone()) {
            if &current_agent == provider_name {
                if let Some(pty_id) = current_pty_id.clone() {
                    let alive = pty.lock().get_session(&pty_id).is_some();
                    if alive {
                        let mut agent_visible = false;
                        for _ in 0..10 {
                            let screen = screen_state.lock().get_screen_text(&pty_id);
                            agent_visible = screen
                                .map(|s| svc.agent_registry.detect_from_output(&s).is_some())
                                .unwrap_or(false);
                            if agent_visible {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                        if agent_visible {
                            let unbound = mux
                                .write()
                                .get_pane(&pid)
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
                            log::info!("[general-agent] Already running (pane={}, pty={}, provider={}) — skipping", pid, pty_id, current_agent);
                            return;
                        }
                        log::info!("[general-agent] Pane has live PTY {} but no agent markers after 3s grace — killing bare shell and relaunching", pty_id);
                        let _ = pty.lock().kill(&pty_id);
                        svc.set_pane_agent_type(&pid, None);
                        if let Some(pane) = mux.write().get_pane_mut(&pid) {
                            pane.pty_id = None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    } else {
                        // Stale pty_id pointing at a dead session — clear it so spawn proceeds cleanly.
                        log::info!(
                            "[general-agent] Clearing stale pty_id {} (session not in PtyManager)",
                            pty_id
                        );
                        if let Some(pane) = mux.write().get_pane_mut(&pid) {
                            pane.pty_id = None;
                        }
                    }
                }
            } else {
                log::info!(
                    "[general-agent] Provider mismatch: running={}, wanted={} — restarting",
                    current_agent,
                    provider_name
                );
                if let Some(pty_id) = current_pty_id {
                    let _ = pty.lock().kill(&pty_id);
                }
                svc.set_pane_agent_type(&pid, None);
                svc.set_agent_session_id(&pid, None);
                state::clear_session_id();
                if let Some(pane) = mux.write().get_pane_mut(&pid) {
                    pane.pty_id = None;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
    }

    // Find General pane (already created by ensure_general_workspace in init_states)
    let (pane_id, snapshot_session_id) = {
        let m = mux.write();
        let pane = m.resolve_pane_id(GENERAL_ALIAS).or_else(|| {
            m.list_workspaces()
                .iter()
                .find(|ws| ws.protected)
                .and_then(|ws| ws.pane_ids.first().cloned())
        });
        match pane {
            Some(pid) => {
                let sid = m.get_pane(&pid).and_then(|p| p.agent_session_id.clone());
                (pid, sid)
            }
            None => {
                log::error!(
                    "[general-agent] No General pane found — ensure_general_workspace failed?"
                );
                return;
            }
        }
    };
    let session_id = if snapshot_session_id.is_some() {
        snapshot_session_id
    } else if let Some(stored) =
        state::load_session_id(&svc.agent_registry, provider_name, &general_cwd)
    {
        svc.set_agent_session_id(&pane_id, Some(stored.clone()));
        log::info!("[general-agent] Loaded durable session id: {}", stored);
        Some(stored)
    } else {
        let exclude = mux.write().other_session_ids(&pane_id);
        let detected = state::detect_owned_session_id(
            &svc.agent_registry,
            provider_name,
            &general_cwd,
            &exclude,
            Some(std::time::Duration::from_secs(86400 * 7)), // 7 days max age
        );
        if let Some(ref sid) = detected {
            svc.set_agent_session_id(&pane_id, Some(sid.clone()));
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
            svc.set_agent_session_id(&pane_id, None);
            state::clear_session_id();
            let exclude = mux.write().other_session_ids(&pane_id);
            let fresh = state::detect_owned_session_id(
                &svc.agent_registry,
                provider_name,
                &general_cwd,
                &exclude,
                Some(std::time::Duration::from_secs(86400 * 7)),
            );
            if let Some(ref sid) = fresh {
                svc.set_agent_session_id(&pane_id, Some(sid.clone()));
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
        general_agent_env_vars(&pane_id, svc.ensure_session_binding_nonce(&pane_id), config);

    let pty_id = {
        let mut manager = pty.lock();
        match manager.spawn_with_id(None, None, Some(general_cwd), cols, rows, env_vars) {
            Ok((id, _)) => id,
            Err(e) => {
                log::error!("[general-agent] Failed to spawn PTY: {}", e);
                return;
            }
        }
    };

    let displaced = {
        let mut m = mux.write();
        let d = m.link_pty(&pane_id, pty_id.clone());
        if let Some(pane) = m.get_pane_mut(&pane_id) {
            pane.auto_approve = true;
        }
        d
    };
    crate::service::orchestrator::pane_relink_signal::notify_pane_relinked();
    if let Some(orphan) = displaced {
        svc.kill_ptys(&[orphan]);
    }
    svc.set_and_lock_pane_agent_type(&pane_id, provider_name.to_string());
    // writes go to the wrong one.
    svc.emit_state_changed();
    log::info!("[general-agent] PTY {} linked to pane {}", pty_id, pane_id);

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

    // reader we just started) every 300ms, returns true when a shell prompt appears on the last non-empty line.
    let ready = crate::service::pty_io::poll_shell_prompt(
        screen_state,
        &pty_id,
        std::time::Duration::from_secs(60),
    );
    if !ready {
        log::warn!(
            "[general-agent] Shell not ready within 60s for PTY {} — watchdog will retry",
            pty_id
        );
        return;
    }
    log::info!("[general-agent] Shell ready, building launch command...");

    // → false). Single-source-of-truth comes from the AgentProvider trait.
    let preserves_context = svc
        .agent_registry
        .with_provider(provider_name, |p| p.preserves_context_on_resume())
        .unwrap_or(false);
    let use_resume = session_id.is_some() && preserves_context;
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
        // This avoids guessing by mtime after start, especially when multiple panes share a cwd.
        let injection = svc
            .agent_registry
            .with_provider(provider_name, |p| p.session_id_injection())
            .unwrap_or(crate::agent::provider::SessionIdInjection::None);
        let pre_session_id = if injection != crate::agent::provider::SessionIdInjection::None {
            let id = uuid::Uuid::new_v4().to_string();
            svc.set_agent_session_id(&pane_id, Some(id.clone()));
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
            .get_pane(&pane_id)
            .and_then(|p| p.cwd.clone())
            .unwrap_or_default();
        let params = crate::agent::provider::AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            session_id: pre_session_id,
            structured_prompt_server: crate::service::structured_prompt::ensure_for_spawn(
                svc,
                provider_name,
                &pane_id,
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
    #[cfg(windows)]
    crate::service::pty_io::poll_shell_prompt(
        screen_state,
        &pty_id,
        std::time::Duration::from_secs(10),
    );
    let payload: Vec<u8> = if cfg!(windows) {
        // Bracketed paste + sacrificial space absorbs PSReadLine/ConPTY first-byte race on Windows.
        format!("\x1b[200~ {}\x1b[201~\r", cmd).into_bytes()
    } else {
        // No bracket-paste: zsh ZLE binds the handler only after the first interactive cycle, so the markers leak as literal text on launch.
        format!("{}\r", cmd).into_bytes()
    };
    if let Err(e) = pty.lock().write(&pty_id, &payload) {
        log::error!("[general-agent] Failed to write launch command: {}", e);
        return;
    }

    // Poll screen for agent detection (up to 15s — Claude can take a while)
    let launched = poll_agent_started(
        screen_state,
        &svc.agent_registry,
        &pty_id,
        std::time::Duration::from_secs(15),
    );
    if !launched {
        log::warn!("[general-agent] Agent not detected within 15s — watchdog will retry if needed");
    }

    if !pane_still_owns_pty(mux, &pane_id, &pty_id) {
        log::info!(
            "[general-agent] Launch on stale PTY {} for pane {} ignored; pane moved on",
            pty_id,
            pane_id
        );
        return;
    }

    // Fresh session: send one inline orient message to seed context.
    // Resume mode omits this because `--resume` already restores prior turns.
    if launched && !use_resume {
        std::thread::sleep(std::time::Duration::from_millis(800));
        let init_msg = "init [Orient yourself: \
            check the pre-injected `## Live mem snapshot` for identity / critical facts; \
            run `codeterm mesh peers`; \
            ack readiness in ONE short line stating your name from L0 and peer count \
            (e.g. \"Ready. <your-name>, 2 peers paired (1 online).\"). \
            Read-only — no spawns, no Telegram, no file writes during init unless mem says so. \
            On first-ever boot: check `bootstrap.md` in CWD for `<!-- bootstrap: true -->` and follow that flow.]";
        if let Err(e) = svc.send_text(&pane_id, init_msg, true) {
            log::warn!("[general-agent] Failed to send init kick: {}", e);
        } else {
            log::info!("[general-agent] Sent fresh-session init kick");
        }
    }

    // no longer inject --session-id), so the next restart can resume this exact session.
    if !use_resume {
        for _ in 0..20 {
            let bound = mux
                .write()
                .get_pane(&pane_id)
                .and_then(|p| p.agent_session_id.clone());
            if let Some(sid) = bound {
                state::persist_session_id(provider_name, &sid);
                log::info!("[general-agent] Persisted hook-bound session {}", sid);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    svc.emit_boot_stage(&pane_id, BootStage::AgentReady, None);
    if let Some(pane) = mux.write().get_pane_mut(&pane_id) {
        pane.agent_booting = false;
    }
    svc.emit_state_changed();
    log::info!(
        "[general-agent] Started successfully (pane={}, pty={})",
        pane_id,
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

    let mut teardown = None;
    let mut general_pane_id = None;
    {
        let m = mux.write();
        if let Some(pid) = m.resolve_pane_id(GENERAL_ALIAS) {
            general_pane_id = Some(pid.clone());
            if let Some(pane) = m.get_pane(&pid) {
                if let Some(ref pty_id) = pane.pty_id {
                    teardown = Some(pty.lock().begin_kill(pty_id));
                    log::info!(
                        "[general-agent] Terminating PTY {} process tree for restart",
                        pty_id
                    );
                }
            }
            drop(m);
            svc.set_pane_agent_type(&pid, None);
            svc.set_agent_session_id(&pid, None);
            state::clear_session_id();
            if let Some(pane) = mux.write().get_pane_mut(&pid) {
                pane.pty_id = None;
            }
        }
    }

    if let Some(teardown) = teardown {
        let result =
            teardown.and_then(|reaper| reaper.wait_bounded(std::time::Duration::from_secs(5)));
        if let Err(reason) = result {
            log::error!("[general-agent] Restart blocked by PTY teardown: {reason}");
            if let Some(pane_id) = general_pane_id {
                svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&reason));
                if let Some(pane) = mux.write().get_pane_mut(&pane_id) {
                    pane.agent_booting = false;
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

pub fn restart_general_agent_pane(
    pane_id: &str,
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    if validate_general_agent_pane_retry(pane_id, mux)? {
        restart_general_agent(svc, mux, pty, screen_state, config, app);
    }
    Ok(())
}

pub fn validate_general_agent_pane_retry(
    pane_id: &str,
    mux: &crate::MuxState,
) -> Result<bool, String> {
    validate_general_agent_pane_retry_inner(pane_id, mux, false)
}

pub fn claim_general_agent_pane_retry(
    pane_id: &str,
    mux: &crate::MuxState,
) -> Result<bool, String> {
    validate_general_agent_pane_retry_inner(pane_id, mux, true)
}

fn validate_general_agent_pane_retry_inner(
    pane_id: &str,
    mux: &crate::MuxState,
    claim: bool,
) -> Result<bool, String> {
    let mut m = mux.write();
    let general_pane_id = m
        .resolve_pane_id(GENERAL_ALIAS)
        .ok_or_else(|| "General Agent pane is not registered".to_string())?;
    let requested_pane_id = m
        .resolve_pane_id(pane_id)
        .unwrap_or_else(|| pane_id.to_string());
    if requested_pane_id != general_pane_id {
        return Err(format!(
            "Retry rejected: pane {} is not the General Agent pane {}",
            requested_pane_id, general_pane_id
        ));
    }
    let pane = m
        .get_pane(&general_pane_id)
        .ok_or_else(|| format!("General Agent pane {} is missing", general_pane_id))?;
    if !pane.protected {
        return Err(format!(
            "Retry rejected: General Agent pane {} is not protected",
            general_pane_id
        ));
    }
    if pane.agent_booting || pane.boot_stage.is_some_and(is_launch_stage) {
        log::info!(
            "[general-agent] Retry for pane {} ignored; launch already in progress",
            general_pane_id
        );
        return Ok(false);
    }
    if claim {
        let _ = pane;
        if let Some(pane) = m.get_pane_mut(&general_pane_id) {
            pane.agent_booting = true;
            pane.boot_stage = Some(BootStage::PtyStarting);
            pane.boot_failure_reason = None;
        }
    }
    Ok(true)
}

fn is_launch_stage(stage: BootStage) -> bool {
    matches!(
        stage,
        BootStage::PtyStarting
            | BootStage::PtySpawned
            | BootStage::ShellReady
            | BootStage::AgentLaunching
            | BootStage::PreparingRoleInstructions
            | BootStage::DeliveringInstructions
    )
}

fn pane_still_owns_pty(mux: &crate::MuxState, pane_id: &str, pty_id: &str) -> bool {
    mux.write()
        .get_pane(pane_id)
        .and_then(|p| p.pty_id.as_deref())
        == Some(pty_id)
}

/// Poll screen_state for signs that the agent has started.
/// Checks screen text against the agent registry's output detection.
pub(super) fn poll_agent_started(
    screen_state: &crate::ScreenStateHandle,
    registry: &crate::agent::registry::AgentRegistry,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() > deadline {
            return false;
        }
        let detected = screen_state
            .try_lock()
            .and_then(|ss| {
                ss.get_screen_text(pty_id)
                    .map(|screen| registry.detect_from_output(&screen).is_some())
            })
            .unwrap_or(false);
        if detected {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn general_agent_env_vars(
    pane_id: &str,
    session_binding_nonce: String,
    config: &crate::config::ConfigState,
) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    env.insert("CODETERM_PANE_ID".to_string(), pane_id.to_string());
    // Per-launch binding nonce so the session-start hook report can prove it owns this pane.
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
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_types::pty::BootStage;
    use codeterm_util::lock_hold::TimedRwLock;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn mux_with_general_pane(stage: Option<BootStage>) -> (MuxState, String) {
        let mut mux = MultiplexerState::empty();
        super::super::workspace::ensure_general_workspace(&mut mux, true);
        let pane_id = mux
            .resolve_pane_id(super::GENERAL_ALIAS)
            .expect("general pane");
        if let Some(pane) = mux.get_pane_mut(&pane_id) {
            pane.boot_stage = stage;
        }
        (Arc::new(TimedRwLock::new("mux", mux)), pane_id)
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

        let env = super::general_agent_env_vars("pane-1", "nonce-1".to_string(), &config);
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
    fn pane_scoped_retry_rejects_non_general_pane() {
        let (mux, general_id) = mux_with_general_pane(Some(BootStage::SpawnFailed));

        let err = super::validate_general_agent_pane_retry("other-pane", &mux).unwrap_err();

        assert!(err.contains("other-pane"), "{err}");
        assert!(err.contains(&general_id), "{err}");
    }

    #[test]
    fn pane_scoped_retry_noops_while_launch_is_in_progress() {
        let (mux, general_id) = mux_with_general_pane(Some(BootStage::PtyStarting));

        let should_restart = super::validate_general_agent_pane_retry(&general_id, &mux).unwrap();

        assert!(!should_restart);
    }

    #[test]
    fn pane_scoped_retry_allows_failed_general_pane() {
        let (mux, general_id) = mux_with_general_pane(Some(BootStage::SpawnFailed));

        let should_restart = super::validate_general_agent_pane_retry(&general_id, &mux).unwrap();

        assert!(should_restart);
    }

    #[test]
    fn pane_scoped_retry_claim_is_single_owner() {
        let (mux, general_id) = mux_with_general_pane(Some(BootStage::SpawnFailed));

        let first = super::claim_general_agent_pane_retry(&general_id, &mux).unwrap();
        let second = super::claim_general_agent_pane_retry(&general_id, &mux).unwrap();

        assert!(first);
        assert!(!second);
        let mux = mux.read();
        let pane = mux.get_pane(&general_id).expect("general pane");
        assert!(pane.agent_booting);
        assert_eq!(pane.boot_stage, Some(BootStage::PtyStarting));
        assert!(pane.boot_failure_reason.is_none());
    }
}
