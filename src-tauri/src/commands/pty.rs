use std::collections::HashMap;
use std::io::Read;
use tauri::AppHandle;

use crate::multiplexer::commands::MuxState;
use crate::service::{boot_stage::BootStage, ServiceState};
use crate::{AgentTrackerState, OutputBufferState, PtyState, ScreenStateHandle};

#[tauri::command]
pub async fn spawn_pty(
    config_state: tauri::State<'_, crate::config::ConfigState>,
    svc: tauri::State<'_, ServiceState>,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    tab_id: Option<String>,
) -> Result<String, String> {
    spawn_pty_core(
        &svc,
        &config_state,
        command,
        cwd,
        cols,
        rows,
        predicted_id,
        tab_id,
    )
    .await
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/spawn.
pub async fn spawn_pty_core(
    svc: &ServiceState,
    config_state: &crate::config::ConfigState,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    tab_id: Option<String>,
) -> Result<String, String> {
    let mut pty_claim = match tab_id.clone() {
        Some(id) => Some(svc.pty_ownership.acquire_async(id).await?),
        None => None,
    };
    let state = &svc.pty;
    let mux_state = &svc.mux;
    let screen_state = &svc.screen_state;
    // Protected tabs reuse their backend PTY; wait briefly to avoid duplicate launch.
    if let Some(ref pid) = tab_id {
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
                let selected = svc.link_pty_with_claim(
                    pty_claim.as_ref().expect("Tab claim"),
                    pid,
                    existing,
                    false,
                );
                if !selected.linked {
                    return Err(format!("Tab {} closed before PTY link", pid));
                }
                log::info!(
                    "[spawn_pty] Tab {} already has live pty {} — reusing",
                    pid,
                    selected.pty_id,
                );
                return Ok(selected.pty_id);
            }
        }
        if is_protected {
            drop(pty_claim.take());
            let svc_wait = svc.clone();
            let tab_wait = pid.clone();
            let relinked = tokio::task::spawn_blocking(move || {
                svc_wait.await_pty_link_blocking(&tab_wait, std::time::Duration::from_secs(5))
            })
            .await
            .map_err(|error| format!("protected Tab relink waiter failed: {error}"))?;
            if let Ok(existing) = relinked {
                return Ok(existing);
            }
            log::warn!(
                "[spawn_pty] protected Tab {} still has no backend pty after 5s — falling through",
                pid,
            );
            pty_claim = Some(svc.pty_ownership.acquire_async(pid.clone()).await?);
        }
    }

    let mut env_vars: HashMap<String, String> = HashMap::new();
    if let Some(ref pid) = tab_id {
        env_vars.insert("CODETERM_TAB_ID".to_string(), pid.clone());
        // Binding nonce so the agent's session_start hook can claim this tab (parity with tab_env).
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".to_string(),
            svc.ensure_session_binding_nonce(pid),
        );
        // Per-session shell-integration nonce — the injected shell script stamps it onto OSC 633;E.
        env_vars.insert(
            "CODETERM_SHELL_NONCE".to_string(),
            svc.ensure_shell_integration_nonce(pid),
        );
        let mux = mux_state.read();
        if let Some(tab) = mux.get_tab(pid) {
            if let Some(ref role) = tab.role {
                env_vars.insert("CODETERM_ROLE".to_string(), format!("{:?}", role));
            }
            if let Some(ref parent) = tab.parent_tab_id {
                env_vars.insert("CODETERM_PARENT_TAB_ID".to_string(), parent.clone());
            }
            if let Some(ref wid) = tab.worker_id {
                env_vars.insert("CODETERM_WORKER_ID".to_string(), wid.clone());
            }
        }
        drop(mux);
    }
    env_vars.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::socket::server::socket_display_path(),
    );
    env_vars.insert(
        "CODETERM_VERSION".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    // URL/port/token for REST callbacks — derived strictly from this instance's
    // config, never inherited process env. Setting CODETERM_API_URL explicitly is
    // what stops a prod-inherited URL (7685) from leaking into the tab and 401ing
    // this instance's bearer against prod.
    let api_cfg = config_state.lock().api.clone();
    let (api_url, api_port) = crate::config::api_callback_url(&api_cfg);
    env_vars.insert("CODETERM_API_URL".into(), api_url);
    env_vars.insert("CODETERM_API_PORT".into(), api_port);
    if let Some(ref tok) = api_cfg.token {
        if !tok.is_empty() {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }
    }

    let t0 = std::time::Instant::now();
    let spawn_gate = state.lock().spawn_gate();
    let built = tokio::task::spawn_blocking(move || {
        crate::pty::manager::build_session(spawn_gate, command, cwd, cols, rows, env_vars)
    })
    .await
    .map_err(|e| format!("spawn_pty join error: {}", e))??;
    let build_ms = t0.elapsed().as_millis();

    let (mut pty_id, _session) = state.lock().register_session(predicted_id, built);
    svc.screen_state.lock().resize(&pty_id, cols, rows);
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
        let Some(restore) = crate::service::pty_io::claim_restored_pty_boot(mux_state, pid)? else {
            return Ok(pty_id);
        };
        let session_id = restore.session_id;
        let tab_cwd = restore.cwd;
        let agent_type = restore.provider_id;
        let tab_model = restore.model;
        let tab_role = restore.role;
        let tab_role_profile = restore.role_profile;
        let mut should_fresh_launch = true;
        let agent_type_for_fresh = agent_type.clone();
        svc.emit_boot_stage(pid, BootStage::PtySpawned, None);

        // Resume only restored snapshots; direct spawn uses backend start.
        if let Some(session_id) = session_id {
            let provider_id = agent_type;

            // Clear stale id if session is gone to avoid resume loop.
            let session_valid = tab_cwd
                .as_deref()
                .map(|cwd| {
                    svc.agent_registry
                        .with_provider(&provider_id, |p| p.session_exists(cwd, &session_id))
                        .unwrap_or(false)
                })
                .unwrap_or(false);

            if !session_valid {
                log::info!(
                    "[resume] session {} for Tab {} has no backing file under {:?} — clearing, will try fresh launch",
                    session_id, pid, tab_cwd
                );
                svc.set_agent_session_id(pid, None);
                svc.emit_state_changed();
            } else {
                svc.set_and_lock_tab_agent_type(pid, provider_id.clone());
                svc.emit_state_changed();

                // Build resume command from provider and global fallback config.
                let pc = crate::config::provider_config(&provider_id);
                let _ = config_state; // global lock no longer needed for these fields
                let resume_enabled = pc.resume_sessions.unwrap_or(true);
                // The global "resume with --dangerously-skip-permissions" toggle is the user's explicit intent for restored agents; per-tab auto_approve is unreliable across spawn paths, so do NOT gate on it.
                let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
                let resume_cmd = svc
                    .agent_registry
                    .with_provider(&provider_id, |p| {
                        let base = p.build_resume_command(&session_id, skip_perms)?;
                        // Preserve the tab's model across restart — bare resume otherwise drops it to the provider default.
                        match tab_model.as_deref().filter(|m| !m.trim().is_empty()) {
                            Some(m) => {
                                let margs = p.model_launch_args(m);
                                Some(if margs.is_empty() {
                                    base
                                } else {
                                    format!("{} {}", base, margs.join(" "))
                                })
                            }
                            None => Some(base),
                        }
                    })
                    .flatten();

                if let Some(cmd) = resume_cmd {
                    let pty_id_for_poll = pty_id.clone();
                    let pty_id_for_launch = pty_id.clone();
                    let screen_state_clone = screen_state.clone();
                    let svc_for_launch = svc.clone();
                    let tab_id_for_boot = pid.clone();
                    let provider_for_ready = provider_id.clone();
                    tokio::spawn(async move {
                        if !resume_enabled {
                            if let Some(tab) =
                                svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                            {
                                tab.agent_booting = false;
                            }
                            svc_for_launch.clear_tab_restored_flag(&tab_id_for_boot);
                            svc_for_launch.emit_boot_stage(
                                &tab_id_for_boot,
                                BootStage::AgentReady,
                                None,
                            );
                            return;
                        }

                        let shell_ready = tokio::task::spawn_blocking(move || {
                            crate::service::pty_io::poll_bare_shell_prompt(
                                &screen_state_clone,
                                &pty_id_for_poll,
                                std::time::Duration::from_secs(60),
                            )
                        })
                        .await
                        .unwrap_or(false);
                        if !shell_ready {
                            if let Some(tab) =
                                svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                            {
                                tab.agent_booting = false;
                            }
                            svc_for_launch.emit_boot_stage(
                                &tab_id_for_boot,
                                BootStage::SpawnFailed,
                                Some("shell did not become ready for session resume"),
                            );
                            return;
                        }

                        log::info!("[resume] sending: {}", cmd);
                        svc_for_launch.emit_boot_stage(
                            &tab_id_for_boot,
                            BootStage::ShellReady,
                            None,
                        );
                        svc_for_launch.emit_boot_stage(
                            &tab_id_for_boot,
                            BootStage::AgentLaunching,
                            None,
                        );
                        let svc_for_blocking = svc_for_launch.clone();
                        let launch_result = tokio::task::spawn_blocking(move || {
                            svc_for_blocking
                                .send_shell_spawn_after_ready(&pty_id_for_launch, &cmd)?;
                            Ok::<bool, String>(svc_for_blocking.await_agent_tui_frame_on_screen(
                                &pty_id_for_launch,
                                &provider_for_ready,
                                std::time::Duration::from_secs(30),
                            ))
                        })
                        .await;
                        match launch_result {
                            Ok(Ok(true)) => {
                                if let Some(tab) =
                                    svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                                {
                                    tab.agent_booting = false;
                                }
                                svc_for_launch.clear_tab_restored_flag(&tab_id_for_boot);
                                svc_for_launch.emit_boot_stage(
                                    &tab_id_for_boot,
                                    BootStage::AgentReady,
                                    None,
                                );
                            }
                            Ok(Ok(false)) => {
                                if let Some(tab) =
                                    svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                                {
                                    tab.agent_booting = false;
                                }
                                svc_for_launch.emit_boot_stage(
                                    &tab_id_for_boot,
                                    BootStage::SpawnFailed,
                                    Some("provider TUI did not become ready after session resume"),
                                );
                            }
                            Ok(Err(e)) => {
                                log::error!("[resume] launch write failed: {}", e);
                                if let Some(tab) =
                                    svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                                {
                                    tab.agent_booting = false;
                                }
                                svc_for_launch.emit_boot_stage(
                                    &tab_id_for_boot,
                                    BootStage::SpawnFailed,
                                    Some(&format!("resume launch write failed: {}", e)),
                                );
                            }
                            Err(e) => {
                                log::error!("[resume] launch task failed: {}", e);
                                if let Some(tab) =
                                    svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                                {
                                    tab.agent_booting = false;
                                }
                                svc_for_launch.emit_boot_stage(
                                    &tab_id_for_boot,
                                    BootStage::SpawnFailed,
                                    Some(&format!("resume launch task failed: {}", e)),
                                );
                            }
                        }
                    });
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
            let pc = crate::config::provider_config(&provider_id);
            let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
            let launch_task = crate::service::provider_launch_task(
                &svc.agent_registry,
                &provider_id,
                None,
                tab_role.as_deref(),
                tab_role_profile.as_deref(),
            );
            let params = crate::service::SpawnAgentParams {
                agent: Some(provider_id.clone()),
                cwd: tab_cwd.clone(),
                skip_permissions: Some(skip_perms),
                model: tab_model.clone(),
                role: tab_role.clone(),
                role_profile: tab_role_profile.clone(),
                starter_prompt: crate::service::starter_prompt_for_role(tab_role.as_deref()),
                ..Default::default()
            };
            let launch_cmd = svc.agent_registry.with_provider(&provider_id, |_| {
                crate::service::build_agent_command(
                    &provider_id,
                    &params,
                    launch_task.as_deref(),
                    None,
                    &svc.agent_registry,
                )
            });
            if let Some(cmd) = launch_cmd {
                let prompt_delivery = svc
                    .agent_registry
                    .with_provider(&provider_id, |provider| {
                        match provider.system_prompt_delivery() {
                            crate::agent::provider::SystemPromptDelivery::CliFlag(_) => "cli_flag",
                            crate::agent::provider::SystemPromptDelivery::SessionConfig => {
                                "session_config"
                            }
                            crate::agent::provider::SystemPromptDelivery::External => {
                                "confirmed_fallback"
                            }
                        }
                    })
                    .unwrap_or("unknown");
                crate::service::launch_diagnostics::log_launch(
                    crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
                    pid,
                    &provider_id,
                    &cmd,
                    launch_task.as_deref(),
                    tab_role.as_deref(),
                    tab_role_profile.as_deref(),
                    prompt_delivery,
                );
                let pty_id_for_poll = pty_id.clone();
                let pty_id_for_launch = pty_id.clone();
                let screen_state_clone = screen_state.clone();
                let provider_label = provider_id.clone();
                let svc_for_launch = svc.clone();
                let tab_id_for_boot = pid.clone();
                tokio::spawn(async move {
                    let shell_ready = tokio::task::spawn_blocking(move || {
                        crate::service::pty_io::poll_bare_shell_prompt(
                            &screen_state_clone,
                            &pty_id_for_poll,
                            std::time::Duration::from_secs(60),
                        )
                    })
                    .await
                    .unwrap_or(false);
                    if !shell_ready {
                        if let Some(tab) = svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                        {
                            tab.agent_booting = false;
                        }
                        svc_for_launch.emit_boot_stage(
                            &tab_id_for_boot,
                            BootStage::SpawnFailed,
                            Some("shell did not become ready for restored fresh launch"),
                        );
                        return;
                    }
                    log::info!(
                        "[restore-fresh] launching {} (no session): {}",
                        provider_label,
                        cmd
                    );
                    svc_for_launch.emit_boot_stage(&tab_id_for_boot, BootStage::ShellReady, None);
                    svc_for_launch.emit_boot_stage(
                        &tab_id_for_boot,
                        BootStage::AgentLaunching,
                        None,
                    );
                    let svc_for_blocking = svc_for_launch.clone();
                    let provider_for_ready = provider_label.clone();
                    let launch_result = tokio::task::spawn_blocking(move || {
                        svc_for_blocking.send_shell_spawn_after_ready(&pty_id_for_launch, &cmd)?;
                        Ok::<bool, String>(svc_for_blocking.await_agent_tui_frame_on_screen(
                            &pty_id_for_launch,
                            &provider_for_ready,
                            std::time::Duration::from_secs(30),
                        ))
                    })
                    .await;
                    match launch_result {
                        Ok(Ok(true)) => {
                            if let Some(tab) =
                                svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                            {
                                tab.agent_booting = false;
                            }
                            svc_for_launch.clear_tab_restored_flag(&tab_id_for_boot);
                            svc_for_launch.emit_boot_stage(
                                &tab_id_for_boot,
                                BootStage::AgentReady,
                                None,
                            );
                        }
                        Ok(Ok(false)) => {
                            if let Some(tab) =
                                svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                            {
                                tab.agent_booting = false;
                            }
                            svc_for_launch.emit_boot_stage(
                                &tab_id_for_boot,
                                BootStage::SpawnFailed,
                                Some("provider TUI did not become ready after fresh restore"),
                            );
                        }
                        Ok(Err(e)) => {
                            log::error!(
                                "[restore-fresh] launch write failed for {} tab: {}",
                                provider_label,
                                e
                            );
                            if let Some(tab) =
                                svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                            {
                                tab.agent_booting = false;
                            }
                            svc_for_launch.emit_boot_stage(
                                &tab_id_for_boot,
                                BootStage::SpawnFailed,
                                Some(&format!("restored fresh launch write failed: {}", e)),
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "[restore-fresh] launch task failed for {} tab: {}",
                                provider_label,
                                e
                            );
                            if let Some(tab) =
                                svc_for_launch.mux.write().get_tab_mut(&tab_id_for_boot)
                            {
                                tab.agent_booting = false;
                            }
                            svc_for_launch.emit_boot_stage(
                                &tab_id_for_boot,
                                BootStage::SpawnFailed,
                                Some(&format!("restored fresh launch task failed: {}", e)),
                            );
                        }
                    }
                });
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

#[tauri::command]
pub async fn read_pty(
    state: tauri::State<'_, PtyState>,
    output_buf: tauri::State<'_, OutputBufferState>,
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    tracker: tauri::State<'_, AgentTrackerState>,
    svc: tauri::State<'_, ServiceState>,
    app: AppHandle,
    pty_id: String,
) -> Result<Vec<u8>, String> {
    let _ = (&state, &output_buf, &screen_state, &mux, &tracker, &app);
    read_pty_core(&svc, pty_id).await
}

/// Long-poll one chunk of PTY output. Shared by the Tauri command and the
/// web mirror's GET /api/pty/{id}/read — every state it needs lives on
/// ServiceContext.
pub async fn read_pty_core(svc: &ServiceState, pty_id: String) -> Result<Vec<u8>, String> {
    let state = &svc.pty;
    let output_buf = &svc.output_buf;
    let mux = &svc.mux;
    let tracker = &svc.agent_tracker;
    let app = &svc.app_handle;
    let session = {
        let manager = state.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    // Prefer backend-reader output when available; it handles decoding/state transitions.
    {
        let rx_opt = session.backend_reader_rx.lock().unwrap().take();
        if let Some(mut rx) = rx_opt {
            let result = rx.recv().await;
            match result {
                Some(mut data) => {
                    if let Some(mark) = crate::pty::manager::take_spawn_time(&pty_id) {
                        log::info!(
                            "[pty-first-byte] pty={} elapsed={}ms bytes={} (via backend channel)",
                            &pty_id[..pty_id.len().min(8)],
                            mark.when.elapsed().as_millis(),
                            data.len()
                        );
                    }
                    coalesce_backend_chunks(&mut rx, &mut data);
                    *session.backend_reader_rx.lock().unwrap() = Some(rx);
                    return Ok(data);
                }
                None => {
                    *session.backend_reader_rx.lock().unwrap() = None;
                    return Err("PTY closed".to_string());
                }
            }
        }
    }

    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            let backend_active = { svc.pty.lock().has_backend_reader(&pty_id) };
            if !backend_active {
                break;
            }
            let rx_ready = session.backend_reader_rx.lock().unwrap().is_some();
            if rx_ready {
                let rx_opt = session.backend_reader_rx.lock().unwrap().take();
                if let Some(mut rx) = rx_opt {
                    let result = rx.recv().await;
                    match result {
                        Some(mut data) => {
                            coalesce_backend_chunks(&mut rx, &mut data);
                            *session.backend_reader_rx.lock().unwrap() = Some(rx);
                            return Ok(data);
                        }
                        None => {
                            *session.backend_reader_rx.lock().unwrap() = None;
                            return Err("PTY closed".to_string());
                        }
                    }
                }
            }
            if std::time::Instant::now() >= deadline {
                log::warn!(
                    "[read_pty] backend reader active but channel never appeared for {} — falling back to raw clone",
                    &pty_id[..pty_id.len().min(8)]
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    if !svc
        .screen_state
        .lock()
        .bind_pty_generation(&pty_id, session.generation)
    {
        return Err(format!(
            "PTY {} was replaced before raw reader start",
            pty_id
        ));
    }
    let pid = pty_id.clone();

    // One read loop owns the reader; clone fresh via try_clone_reader if taken.
    let mut reader_to_use = {
        let mut reader_slot = session.reader.lock().unwrap();
        match reader_slot.take() {
            Some(r) => r,
            None => {
                let pair = session.pair.lock().unwrap();
                let Some(pair) = pair.as_ref() else {
                    return Ok(Vec::new());
                };
                pair.master.try_clone_reader().map_err(|e| e.to_string())?
            }
        }
    };

    let pid_for_timing = pid.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; 8192];
        let n = reader_to_use.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("EOF".to_string());
        }
        buf.truncate(n);

        Ok((buf, reader_to_use))
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Ok((ref data, _)) = result {
        if let Some(mark) = crate::pty::manager::take_spawn_time(&pid_for_timing) {
            log::info!(
                "[pty-first-byte] pty={} elapsed={}ms bytes={} (via raw reader)",
                &pid_for_timing[..pid_for_timing.len().min(8)],
                mark.when.elapsed().as_millis(),
                data.len()
            );
        }
    }

    match result {
        Ok((data, reader_back)) => {
            let mut reader_slot = session.reader.lock().unwrap();
            *reader_slot = Some(reader_back);

            let (prev, now) =
                svc.feed_pty_output_for_generation(&pty_id, session.generation, &data);
            svc.emit_tab_status_if_changed(&pty_id, prev, now);

            Ok(data)
        }
        Err(e) => {
            let raw_prev = {
                let mut tracker = tracker.lock();
                let prev = tracker.get_status(&pty_id);
                tracker.set_dead(&pty_id);
                prev
            };
            let tab_id = mux.write().tab_id_for_pty(&pty_id);
            if let Some(ref pid) = tab_id {
                let prev = svc.reconciled_tab_status_for_id(pid, raw_prev);
                svc.emit_tab_status_if_changed(
                    &pty_id,
                    prev,
                    crate::agent::status::TabStatus::Dead,
                );

                // Cron fallback marks failed if agent dies without cron completion.
                use tauri::Manager;
                if let Some(cron_state) = app.try_state::<crate::CronState>() {
                    let owner = cron_state.lock().get_tab_owner(pid).cloned();
                    if let Some((job_id, run_id)) = owner {
                        let mut fresh_transition = false;
                        {
                            let mut store = cron_state.lock();
                            // Only save if no log exists yet (don't overwrite clean summary)
                            if store.read_log(&job_id, &run_id).is_none() {
                                let text = output_buf.lock().get_text(&pty_id).unwrap_or_default();
                                let clean = crate::pty::ansi::clean_terminal_output(&text);
                                let _ = store.save_log(&job_id, &run_id, &clean);
                                fresh_transition = store
                                    .update_run_status(
                                        &job_id,
                                        &run_id,
                                        crate::cron::store::CronRunStatus::Failed,
                                    )
                                    .unwrap_or(false);
                            }
                            store.unregister_tab(pid);
                        }
                        // Notify after the store guard drops — it re-locks internally.
                        if fresh_transition {
                            crate::cron::notify::notify_on_consecutive_failure(
                                svc,
                                &cron_state,
                                &job_id,
                            );
                        }
                        log::info!("Cron Tab exited: job={}, run={}", job_id, run_id);
                        crate::api::events::emit_everywhere(&app, "cron-run-finished", ());
                    }
                }
            }
            Err(e)
        }
    }
}

fn coalesce_backend_chunks(rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>, data: &mut Vec<u8>) {
    const COALESCE_CAP: usize = 64 * 1024;

    while data.len() < COALESCE_CAP {
        match rx.try_recv() {
            Ok(mut next) => data.append(&mut next),
            Err(_) => break,
        }
    }
}

#[tauri::command]
pub async fn write_pty_string(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    data: String,
    raw: Option<bool>,
) -> Result<(), String> {
    let _ = (&state, &mux_state);
    if raw.unwrap_or(true) {
        write_pty_string_core(&svc, pty_id, data)
    } else {
        // Bracketed paste with sacrificial space on Windows — see build_spawn_dispatch.
        let svc = svc.inner().clone();
        tokio::task::spawn_blocking(move || {
            let text_stripped = data.trim_end_matches('\r');
            let enter = text_stripped.len() != data.len();
            svc.send_text_to_pty_input(&pty_id, text_stripped, enter)
        })
        .await
        .map_err(|e| format!("send task failed: {e}"))?
    }
}

#[tauri::command]
pub async fn interrupt_agent(
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
) -> Result<(), String> {
    write_pty_string_core(&svc, pty_id, "\x1b".to_string())
}

/// Deliver an agent's initial launch command via the no-verify launch writer
/// (no agent TUI exists pre-boot). poll_shell_prompt blocks → off the runtime.
#[tauri::command]
pub async fn write_pty_launch_command(
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    command: String,
) -> Result<(), String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.send_launch_command_to_pty(&pty_id, &command))
        .await
        .map_err(|e| format!("launch task failed: {e}"))?
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/{id}/write.
pub fn write_pty_string_core(
    svc: &ServiceState,
    pty_id: String,
    data: String,
) -> Result<(), String> {
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
    session.write_queue.send(bytes)
}

#[tauri::command]
pub async fn write_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };
    parse_pty_input(&svc, &mux_state, &pty_id, &data);
    mark_user_activity(&svc, &mux_state, &pty_id);
    crate::pty::capture::CaptureRegistry::global().tap_input(&pty_id, "write_pty", &data);
    mark_manual_agent_interrupt(&svc, &pty_id, &data);
    session.write_queue.send(data)
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
/// On match, lock `agent_type` to the detected provider — user typed an explicit
/// launch command, which is the highest-authority intent next to a real spawn.
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

#[tauri::command]
pub async fn resize_pty(
    state: tauri::State<'_, PtyState>,
    screen_state: tauri::State<'_, ScreenStateHandle>,
    resize_owner: tauri::State<'_, crate::ResizeOwnerState>,
    pty_id: String,
    cols: u16,
    rows: u16,
    client_id: Option<String>,
) -> Result<(), String> {
    if let Some(client_id) = client_id.as_deref() {
        if !resize_owner.lock().is_owner(&pty_id, client_id) {
            return Ok(());
        }
    }
    resize_pty_core(&state, &screen_state, pty_id, cols, rows)
}

#[tauri::command]
pub async fn claim_resize_owner(
    resize_owner: tauri::State<'_, crate::ResizeOwnerState>,
    events: tauri::State<'_, crate::api::events::EventBroadcasterState>,
    tab_id: String,
    pty_id: String,
    client_id: String,
    cols: u16,
    rows: u16,
    force: Option<bool>,
) -> Result<crate::resize_owner::ResizeOwner, String> {
    let owner = resize_owner.lock().claim(
        tab_id,
        pty_id,
        client_id,
        cols,
        rows,
        force.unwrap_or(false),
    );
    events.publish(crate::api::events::SystemEvent::Generic {
        event_type: "pty-resize-owner-changed".to_string(),
        data: serde_json::to_value(&owner).unwrap_or_default(),
    });
    Ok(owner)
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/{id}/resize.
pub fn resize_pty_core(
    state: &PtyState,
    screen_state: &ScreenStateHandle,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let _ = session.resize(cols, rows)?;
    screen_state.lock().resize(&pty_id, cols, rows);
    crate::pty::capture::CaptureRegistry::global().tap_resize(&pty_id, cols, rows);
    Ok(())
}

#[tauri::command]
pub async fn kill_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    pty_id: String,
) -> Result<(), String> {
    // if it targets an already-owned PTY to avoid tearing GA down mid-launch.
    let owned_by_protected = {
        let mux = mux_state.read();
        mux.tab_id_for_pty(&pty_id)
            .and_then(|pid| mux.get_tab(&pid).map(|p| p.protected))
            .unwrap_or(false)
    };
    if owned_by_protected {
        log::info!(
            "[kill_pty] {} is owned by a protected Tab - ignoring frontend kill",
            pty_id,
        );
        return Ok(());
    }
    let mut manager = state.lock();
    kill_pty_idempotent(&mut manager, &pty_id)
}

pub(crate) fn kill_pty_idempotent(
    manager: &mut crate::pty::manager::PtyManager,
    pty_id: &str,
) -> Result<(), String> {
    match manager.kill(pty_id) {
        Ok(()) => Ok(()),
        Err(e) if is_missing_pty_error(pty_id, &e) => Ok(()),
        Err(e) => Err(e),
    }
}

fn is_missing_pty_error(pty_id: &str, error: &str) -> bool {
    error == format!("PTY {} not found", pty_id)
}

#[cfg(test)]
mod kill_pty_tests {
    use std::collections::HashMap;

    use super::{is_missing_pty_error, kill_pty_idempotent};

    #[test]
    fn kill_pty_idempotent_kills_child_and_allows_repeat() {
        let mut manager = crate::pty::manager::PtyManager::new();
        let (pty_id, session) = manager
            .spawn(None, None, 80, 24, HashMap::new())
            .expect("spawn default shell");
        let child_pid = session.child.lock().expect("child lock").process_id();
        assert!(child_pid.is_some(), "spawned child should expose a pid");

        kill_pty_idempotent(&mut manager, &pty_id).expect("first kill succeeds");
        assert!(
            manager.get_session(&pty_id).is_none(),
            "kill must remove session from manager"
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
                "kill must terminate the child"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        kill_pty_idempotent(&mut manager, &pty_id).expect("repeat kill is a no-op");
        kill_pty_idempotent(&mut manager, "missing-pty").expect("unknown kill is a no-op");
    }

    #[test]
    fn kill_pty_idempotent_does_not_swallow_unrelated_errors() {
        let mut manager = crate::pty::manager::PtyManager::new();
        let err = manager
            .kill("real-id")
            .expect_err("manager should expose exact missing-id error");
        assert!(is_missing_pty_error("real-id", &err));
        assert!(!is_missing_pty_error("other-id", &err));
        assert!(!is_missing_pty_error(
            "real-id",
            "PTY real-id not found: access denied"
        ));
        assert!(!is_missing_pty_error(
            "real-id",
            "wrapped PTY real-id not found"
        ));
    }
}
