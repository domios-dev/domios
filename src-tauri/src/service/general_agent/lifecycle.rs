//! Start / restart / readiness-poll for the General Agent PTY+launch flow.

use super::{general_dir, instructions, GENERAL_ALIAS};

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

    // Live PTY alone isn't enough: after snapshot rehydrate the frontend may race-spawn a bare shell into the pane while agent_type is
    // cosmetically restored — verify with screen markers and tear down the bare shell if no agent is running.
    {
        let (pane_id_opt, current_agent, current_pty_id) = {
            let m = mux.lock();
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
                        // Grace-poll markers: after a daemon restart the live PTY
                        // is up but the screen isn't rehydrated yet, so a one-shot
                        // check would kill a healthy Main. Bail the instant markers
                        // appear; only a genuinely bare shell survives the window.
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
                            log::info!("[general-agent] Already running (pane={}, pty={}, provider={}) — skipping", pid, pty_id, current_agent);
                            return;
                        }
                        log::info!("[general-agent] Pane has live PTY {} but no agent markers after 3s grace — killing bare shell and relaunching", pty_id);
                        let _ = pty.lock().kill(&pty_id);
                        svc.set_pane_agent_type(&pid, None);
                        if let Some(pane) = mux.lock().get_pane_mut(&pid) {
                            pane.pty_id = None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    } else {
                        // Stale pty_id pointing at a dead session — clear it so spawn proceeds cleanly.
                        log::info!(
                            "[general-agent] Clearing stale pty_id {} (session not in PtyManager)",
                            pty_id
                        );
                        if let Some(pane) = mux.lock().get_pane_mut(&pid) {
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
                if let Some(pane) = mux.lock().get_pane_mut(&pid) {
                    pane.pty_id = None;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
    }

    // Find General pane (already created by ensure_general_workspace in init_states)
    let (pane_id, session_id) = {
        let m = mux.lock();
        let pane = m.resolve_pane_id(GENERAL_ALIAS).or_else(|| {
            m.list_workspaces()
                .iter()
                .find(|ws| ws.protected)
                .and_then(|ws| ws.pane_ids.first().cloned())
        });
        match pane {
            Some(pid) => {
                let mut sid = m.get_pane(&pid).and_then(|p| p.agent_session_id.clone());
                // If no session_id from snapshot, try detecting from filesystem (covers first launch, new machine, or wiped session store)
                if sid.is_none() {
                    let exclude: Vec<String> = m.other_session_ids(&pid);
                    drop(m); // release lock before filesystem scan
                    sid = crate::multiplexer::commands::detect_agent_session_id(
                        &svc.agent_registry,
                        provider_name,
                        &general_cwd,
                        Some(exclude),
                        Some(86400 * 7), // 7 days max age
                    )
                    .ok()
                    .flatten();
                    if let Some(ref detected) = sid {
                        svc.set_agent_session_id(&pid, Some(detected.clone()));
                        log::info!(
                            "[general-agent] Detected session from filesystem: {}",
                            detected
                        );
                    }
                }
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

    // Validate session_id: if provider can't resolve it to a file, it's stale/wrong-provider
    let session_id = if let Some(ref id) = session_id {
        let valid = svc
            .agent_registry
            .with_provider(provider_name, |p| p.session_exists(&general_cwd, id))
            .unwrap_or(false);
        if !valid {
            log::info!("[general-agent] Session '{}' not valid for provider '{}' — clearing, will detect fresh", id, provider_name);
            svc.set_agent_session_id(&pane_id, None);
            let exclude = mux.lock().other_session_ids(&pane_id);
            let fresh = crate::multiplexer::commands::detect_agent_session_id(
                &svc.agent_registry,
                provider_name,
                &general_cwd,
                Some(exclude),
                Some(86400 * 7),
            )
            .ok()
            .flatten();
            if let Some(ref sid) = fresh {
                svc.set_agent_session_id(&pane_id, Some(sid.clone()));
                log::info!("[general-agent] Detected fresh session: {}", sid);
            }
            fresh
        } else {
            session_id
        }
    } else {
        session_id
    };

    // 5. Spawn PTY
    let cols = 120u16;
    let rows = 40u16;
    let env_vars = {
        let mut env = std::collections::HashMap::new();
        env.insert("CODETERM_PANE_ID".to_string(), pane_id.clone());
        env.insert(
            "CODETERM_SOCKET_PATH".to_string(),
            crate::socket::server::socket_display_path(),
        );
        env.insert("CODETERM_ROLE".to_string(), "Orchestrator".to_string());
        if let Ok(port) = std::env::var("CODETERM_API_PORT") {
            env.insert("CODETERM_API_PORT".to_string(), port);
        }
        // Hook callbacks POST to our REST API. Without the token they'd 401 on any non-loopback bind and silently swallow the failure via `curl … || true`, so set it unconditionally.
        let tok: Option<String> = config.lock().api.token.clone();
        if let Some(tok) = tok {
            if !tok.is_empty() {
                env.insert("CODETERM_API_TOKEN".to_string(), tok);
            }
        }
        env
    };

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
        let mut m = mux.lock();
        let d = m.link_pty(&pane_id, pty_id.clone());
        if let Some(pane) = m.get_pane_mut(&pane_id) {
            pane.auto_approve = true;
        }
        d
    };
    if let Some(orphan) = displaced {
        svc.kill_ptys(&[orphan]);
    }
    svc.set_and_lock_pane_agent_type(&pane_id, provider_name.to_string());
    // Emit state NOW so frontend picks up pty_id immediately — don't wait for agent detection. Without this, frontend auto-spawns its own PTY and
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
                match crate::service::pty_lifecycle::start_backend_reader(&svc_clone, &pty_id_clone)
                {
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

    // Wait for shell to be ready — same mechanism as regular pane resume in commands.rs. poll_shell_prompt polls screen_state (fed by the backend
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

    // Build launch command. Resume only when the provider preserves prior context across resume (Claude replays JSONL → true; Codex restarts fresh
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
            log::info!(
                "[general-agent] Pre-assigned session id {} for fresh launch",
                id
            );
            Some(id)
        } else {
            None
        };
        let params = crate::agent::provider::AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: pre_session_id,
        };
        match svc
            .agent_registry
            .with_provider(provider_name, |p| p.build_launch_command(&params))
        {
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

    // Find general pane and kill its PTY
    {
        let m = mux.lock();
        if let Some(pid) = m.resolve_pane_id(GENERAL_ALIAS) {
            if let Some(pane) = m.get_pane(&pid) {
                if let Some(ref pty_id) = pane.pty_id {
                    let _ = pty.lock().kill(pty_id);
                    log::info!("[general-agent] Killed PTY {} for restart", pty_id);
                }
            }
            drop(m);
            svc.set_pane_agent_type(&pid, None);
            svc.set_agent_session_id(&pid, None);
            if let Some(pane) = mux.lock().get_pane_mut(&pid) {
                pane.pty_id = None;
            }
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Re-generate instructions for potentially new provider in the background. Clone to avoid holding the config lock (deadlock with
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
