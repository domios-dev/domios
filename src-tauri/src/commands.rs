use std::collections::HashMap;
use std::io::Read;
use base64::Engine as _;
use tauri::{AppHandle, Emitter, State};

use crate::agent::git_info::{self, GitInfo};
use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;
use crate::{AgentTrackerState, OutputBufferState, PtyState, ScreenStateHandle, UpdateStateHandle};

#[tauri::command]
pub async fn check_for_update(
    update_state: tauri::State<'_, UpdateStateHandle>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let method = update_state.lock().install_method.clone();
    let info = crate::update::checker::check_for_update(&method)
        .await
        .map_err(|e| e.to_string())?;

    if info.update_available {
        use tauri::Emitter;
        let _ = app.emit("update-available", serde_json::to_value(&info).unwrap_or_default());
    }

    let val = serde_json::to_value(&info).map_err(|e| e.to_string())?;
    let mut us = update_state.lock();
    us.last_check = Some(info);
    us.last_check_time = Some(std::time::Instant::now());
    Ok(val)
}

#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    update_state: tauri::State<'_, UpdateStateHandle>,
) -> Result<(), String> {
    let channel = update_state.lock().channel;
    crate::update::apply::check_and_apply(
        &app,
        crate::update::apply::ApplyOptions {
            channel,
            manifest_url: None,
        },
    )
    .await
}

#[tauri::command]
pub fn get_windows_build_number() -> u32 {
    // Build number is constant for the OS lifetime — cache to avoid spawning
    // cmd.exe on every TerminalPane mount (~1s on Win11 with Defender).
    static CACHED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            if let Ok(output) = Command::new("cmd").args(["/c", "ver"]).output() {
                let ver = String::from_utf8_lossy(&output.stdout);
                if let Some(start) = ver.rfind('.') {
                    let before_last = &ver[..start];
                    if let Some(build_start) = before_last.rfind('.') {
                        if let Ok(n) = before_last[build_start + 1..].trim().parse::<u32>() {
                            return n;
                        }
                    }
                }
            }
            19041
        }
        #[cfg(not(target_os = "windows"))]
        { 0 }
    })
}

#[tauri::command]
pub fn get_default_cwd() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn spawn_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    screen_state: tauri::State<'_, ScreenStateHandle>,
    config_state: tauri::State<'_, crate::config::ConfigState>,
    svc: tauri::State<'_, ServiceState>,
    _app: AppHandle,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    pane_id: Option<String>,
) -> Result<String, String> {
    let mut env_vars: HashMap<String, String> = HashMap::new();
    if let Some(ref pid) = pane_id {
        env_vars.insert("CODETERM_PANE_ID".to_string(), pid.clone());
        // Pass role/parent info so agents know their place in hierarchy
        let mux = mux_state.lock();
        if let Some(pane) = mux.get_pane(pid) {
            if let Some(ref role) = pane.role {
                env_vars.insert("CODETERM_ROLE".to_string(), format!("{:?}", role));
            }
            if let Some(ref parent) = pane.parent_pane_id {
                env_vars.insert("CODETERM_PARENT_PANE_ID".to_string(), parent.clone());
            }
            if let Some(ref wid) = pane.worker_id {
                env_vars.insert("CODETERM_WORKER_ID".to_string(), wid.clone());
            }
        }
        drop(mux);
    }
    env_vars.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::socket::server::socket_display_path(),
    );
    // Hook callbacks (Notification/Stop/PreCompact/SessionStart) POST to our
    // REST API with an Authorization header read from this env var. Without
    // it, the hook curl hits auth_middleware and 401s silently.
    let api_cfg = config_state.lock().api.clone();
    if let Ok(port) = std::env::var("CODETERM_API_PORT") {
        env_vars.insert("CODETERM_API_PORT".into(), port);
    } else {
        env_vars.insert("CODETERM_API_PORT".into(), api_cfg.port.to_string());
    }
    if let Some(ref tok) = api_cfg.token {
        if !tok.is_empty() {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }
    }

    let t0 = std::time::Instant::now();
    let built = tokio::task::spawn_blocking(move || {
        crate::pty::manager::build_session(command, cwd, cols, rows, env_vars)
    })
    .await
    .map_err(|e| format!("spawn_pty join error: {}", e))??;
    let build_ms = t0.elapsed().as_millis();

    let (pty_id, _session) = state.lock().register_session(predicted_id, built);
    log::info!("PTY {} spawned (build={}ms)", pty_id, build_ms);

    if let Some(ref pid) = pane_id {
        svc.link_pty(pid, pty_id.clone());
    }

    // Auto-resume: if this pane has a saved session_id, resume it after shell is ready.
    // This runs in backend so it works even for non-visible tabs.
    // Skip protected panes (General Agent) — backend manages their lifecycle.
    if let Some(ref pid) = pane_id {
        let (session_id, is_protected, pane_cwd) = {
            let m = mux_state.lock();
            let pane = m.get_pane(pid);
            (
                pane.and_then(|p| p.agent_session_id.clone()),
                pane.map(|p| p.protected).unwrap_or(false),
                pane.and_then(|p| p.cwd.clone()),
            )
        };
        let agent_type = mux_state.lock().get_pane(pid)
            .and_then(|p| p.agent_type.clone());
        if let Some(session_id) = session_id.filter(|_| !is_protected) {
            let provider_id = agent_type.unwrap_or_else(crate::config::default_agent_provider);

            // Validate the session file still exists. If it's gone (cleared cache,
            // different machine, /new inside the TUI last run), resuming produces
            // spam like "No conversation found with session ID: ..." which the
            // watchdog then retries forever. Clear and bail quietly instead.
            let session_valid = pane_cwd.as_deref().map(|cwd| {
                svc.agent_registry.with_provider(&provider_id, |p| {
                    p.session_exists(cwd, &session_id)
                }).unwrap_or(false)
            }).unwrap_or(false);

            if !session_valid {
                log::info!(
                    "[resume] session {} for pane {} has no backing file under {:?} — clearing, skipping resume",
                    session_id, pid, pane_cwd
                );
                svc.set_agent_session_id(pid, None);
                svc.emit_state_changed();
                return Ok(pty_id);
            }

            svc.set_pane_agent_type(pid, Some(provider_id.clone()));
            svc.emit_state_changed();

            // Build resume command via provider registry (provider-agnostic).
            // Per-provider config: each plugin has its own resume_sessions /
            // resume_skip_permissions in `[agents.<id>]` (back-compat fallback
            // to global `[agent]` is handled by `provider_config()`).
            let pc = crate::config::provider_config(&provider_id);
            let _ = config_state; // global lock no longer needed for these fields
            let resume_enabled = pc.resume_sessions.unwrap_or(true);
            let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
            let resume_cmd = svc.agent_registry.with_provider(&provider_id, |p| {
                p.build_resume_command(&session_id, skip_perms)
            }).flatten();

            if let Some(cmd) = resume_cmd {
                let pty_clone = state.inner().clone();
                let pty_id_for_poll = pty_id.clone();
                let pty_id_for_write = pty_id.clone();
                let screen_state_clone = screen_state.inner().clone();
                tokio::spawn(async move {
                    if !resume_enabled { return; }

                    // 60s is generous but matters under contention: at app
                    // restart dozens of panes spawn in parallel and shell
                    // initialization can stall well past the old 15s budget
                    // (observed 14-20s for Nth-pane zsh under load on macOS).
                    let ready = tokio::task::spawn_blocking(move || {
                        crate::service::pty_io::poll_shell_prompt(
                            &screen_state_clone, &pty_id_for_poll,
                            std::time::Duration::from_secs(60),
                        )
                    }).await.unwrap_or(false);

                    if !ready {
                        log::warn!("[resume] shell not ready for session {}, skipping", session_id);
                        return;
                    }

                    log::info!("[resume] sending: {}", cmd);
                    let paste = format!("\x1b[200~{}\x1b[201~\r", cmd);
                    let _ = pty_clone.lock().write(&pty_id_for_write, paste.as_bytes());
                });
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
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    // If a backend reader is active, it forwards output via channel to avoid competing reads.
    // Read from channel instead of raw PTY. Processing (OutputBuffer, ScreenState, process_pty_output)
    // is already done by the backend reader — don't duplicate it here.
    {
        let rx_opt = session.backend_reader_rx.lock().unwrap().take();
        if let Some(mut rx) = rx_opt {
            let result = rx.recv().await;
            match result {
                Some(data) => {
                    if let Some(mark) = crate::pty::manager::take_spawn_time(&pty_id) {
                        log::info!(
                            "[pty-first-byte] pty={} elapsed={}ms bytes={} (via backend channel)",
                            &pty_id[..pty_id.len().min(8)],
                            mark.when.elapsed().as_millis(),
                            data.len()
                        );
                    }
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

    let ob = (*output_buf).clone();
    let ss = (*screen_state).clone();
    let pid = pty_id.clone();

    // Take reader from session — only one readLoop can own it at a time.
    // If reader was already taken (by a previous readLoop that's blocking in read()),
    // create a new one via try_clone_reader.
    let mut reader_to_use = {
        let mut reader_slot = session.reader.lock().unwrap();
        match reader_slot.take() {
            Some(r) => r,
            None => {
                // Reader taken by old readLoop — clone a new one from master
                let pair = session.pair.lock().unwrap();
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
        ob.lock().append(&pid, &buf);
        ss.lock().process(&pid, &buf);

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
            // Put reader back for next call
            let mut reader_slot = session.reader.lock().unwrap();
            *reader_slot = Some(reader_back);

            // Status tracking + agent event detection (delegated to service layer)
            let (prev, now) = svc.process_pty_output(&pty_id, &data);

            // Emit status change event to frontend
            if now != prev {
                let pane_id = mux.lock().pane_id_for_pty(&pty_id);
                if let Some(ref pid) = pane_id {
                    use tauri::Emitter;
                    let _ = app.emit("pane-status-changed", serde_json::json!({
                        "pane_id": pid,
                        "status": format!("{:?}", now),
                    }));
                }
            }

            Ok(data)
        }
        Err(e) => {
            // PTY died
            tracker.lock().set_dead(&pty_id);
            let pane_id = mux.lock().pane_id_for_pty(&pty_id);
            if let Some(ref pid) = pane_id {
                use tauri::Emitter;
                let _ = app.emit("pane-status-changed", serde_json::json!({
                    "pane_id": pid,
                    "status": "Dead",
                }));

                // Cron fallback: if agent died without calling cron complete, mark as failed
                use tauri::Manager;
                if let Some(cron_state) = app.try_state::<crate::CronState>() {
                    let owner = cron_state.lock().get_pane_owner(pid).cloned();
                    if let Some((job_id, run_id)) = owner {
                        let mut store = cron_state.lock();
                        // Only save if no log exists yet (don't overwrite clean summary)
                        if store.read_log(&job_id, &run_id).is_none() {
                            let text = output_buf.lock().get_text(&pty_id).unwrap_or_default();
                            let clean = crate::pty::ansi::clean_terminal_output(&text);
                            let _ = store.save_log(&job_id, &run_id, &clean);
                            let _ = store.update_run_status(&job_id, &run_id, crate::cron::store::CronRunStatus::Failed);
                        }
                        store.unregister_pane(pid);
                        log::info!("Cron pane exited: job={}, run={}", job_id, run_id);
                        let _ = app.emit("cron-state-changed", ());
                    }
                }
            }
            Err(e)
        },
    }
}

#[tauri::command]
pub async fn write_pty_string(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    session.write_queue.send(data.into_bytes())
}

#[tauri::command]
pub async fn write_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };
    session.write_queue.send(data)
}

#[tauri::command]
pub async fn resize_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let pair = session.pair.lock().unwrap();
    pair.master.resize(portable_pty::PtySize {
        rows, cols, pixel_width: 0, pixel_height: 0,
    }).map_err(|e| format!("Resize failed: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn kill_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
) -> Result<(), String> {
    let mut manager = state.lock();
    manager.remove(&pty_id);
    Ok(())
}

#[tauri::command]
pub async fn get_pane_output(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
) -> Result<String, String> {
    let cmd = crate::dispatch::Command::GetOutput {
        pane_id,
        offset: None,
        max_bytes: None,
        strip_ansi: true,
        full: true,
    };
    let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
    Ok(result
        .into_json()
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

/// formatted=true preserves ANSI + cursor state (needed for TUI remount).
#[tauri::command]
pub async fn get_pane_text(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
    formatted: Option<bool>,
) -> Result<String, String> {
    let cmd = crate::dispatch::Command::GetText {
        pane_id,
        formatted: formatted.unwrap_or(false),
    };
    let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
    Ok(result
        .into_json()
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

#[tauri::command]
pub async fn list_agent_providers(
    svc: tauri::State<'_, ServiceState>,
) -> Result<serde_json::Value, String> {
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::ListAgentProviders,
        svc.as_ref(),
    )?;
    Ok(result.into_json())
}

#[tauri::command]
pub async fn wait_shell_ready(
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
    timeout_ms: Option<u64>,
) -> Result<bool, String> {
    let pty_id = {
        let m = mux.lock();
        m.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| "Pane not found or has no PTY".to_string())?
    };
    let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(10_000));
    let ss = screen_state.inner().clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::service::pty_io::poll_shell_prompt(&ss, &pty_id, timeout)
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn read_session_jsonl(
    mux: tauri::State<'_, MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
    offset: Option<u64>,
    tail_bytes: Option<u64>,
) -> Result<String, String> {
    // 1. Get agent_session_id, agent_type, and cwd from pane
    let (mut session_id, cwd, agent_type) = {
        let m = mux.lock();
        let pane = m.get_pane(&pane_id)
            .ok_or_else(|| "Pane not found".to_string())?;
        let sid = pane.agent_session_id.clone();
        let cwd = pane.cwd.clone()
            .ok_or_else(|| "Pane has no CWD".to_string())?;
        let at = pane.agent_type.clone().unwrap_or_else(crate::config::default_agent_provider);
        (sid, cwd, at)
    };

    let needs_detect = if session_id.is_none() {
        true
    } else if let Some(ref sid) = session_id {
        svc.agent_registry.with_provider(&agent_type, |p| p.session_file_path(&cwd, sid))
            .flatten()
            .map(|path| !path.exists())
            .unwrap_or(true)
    } else { false };

    if needs_detect {
        let exclude = mux.lock().other_session_ids(&pane_id);
        let max_age = Some(std::time::Duration::from_secs(86400));
        let detected = svc.agent_registry.with_provider(&agent_type, |p| {
            p.detect_session_id(&cwd, &exclude, max_age)
        }).flatten();
        if let Some(detected) = detected {
            log::info!("[read_session_jsonl] detected session {} for pane {}", detected, pane_id);
            svc.set_agent_session_id(&pane_id, Some(detected.clone()));
            svc.emit_state_changed();
            session_id = Some(detected);
        }
    }

    let session_id = session_id
        .ok_or_else(|| "No agent session linked to this pane".to_string())?;

    let jsonl_path = svc.agent_registry.with_provider(&agent_type, |p| p.session_file_path(&cwd, &session_id))
        .flatten()
        .ok_or_else(|| format!("No session file path for agent type: {}", agent_type))?;

    if !jsonl_path.exists() {
        return Err(format!("JSONL not found: {}", jsonl_path.display()));
    }

    use std::io::{Read as IoRead, Seek, SeekFrom};
    let mut file = std::fs::File::open(&jsonl_path).map_err(|e| e.to_string())?;
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();

    let skip = offset.unwrap_or(0);
    if skip > 0 {
        if skip >= file_len {
            return Ok(String::new());
        }
        file.seek(SeekFrom::Start(skip)).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    } else if let Some(tail) = tail_bytes {
        let start = if file_len > tail { file_len - tail } else { 0 };
        file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        if start > 0 {
            if let Some(pos) = buf.find('\n') {
                buf = buf[pos + 1..].to_string();
            }
        }
        Ok(format!("__offset:{}\n{}", file_len, buf))
    } else {
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

fn extract_revision(content: &str) -> u32 {
    content.lines()
        .find_map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with("<!-- revision:") && trimmed.ends_with("-->") {
                let inner = trimmed
                    .trim_start_matches("<!-- revision:")
                    .trim_end_matches("-->")
                    .trim();
                inner.parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn find_bundled_skills_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));

        let adjacent = exe_dir.join("skills");
        if adjacent.exists() { return adjacent; }

        #[cfg(target_os = "macos")]
        {
            let resources = exe_dir.join("../Resources/skills");
            if resources.exists() { return resources.canonicalize().unwrap_or(resources); }
        }

        let dev = exe_dir.join("../../../skills");
        if dev.exists() { return dev.canonicalize().unwrap_or(dev); }

        let dev_deep = exe_dir.join("../../../../skills");
        if dev_deep.exists() { return dev_deep.canonicalize().unwrap_or(dev_deep); }
    }
    std::path::PathBuf::from("skills")
}

pub fn seed_bundled_skills() {
    let bundled = find_bundled_skills_dir();
    let canonical = crate::paths::data_dir().join("skills");
    let _ = std::fs::create_dir_all(&canonical);

    let Ok(entries) = std::fs::read_dir(&bundled) else { return };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
        let id = entry.file_name();
        let src_skill = entry.path().join("SKILL.md");
        let dst_dir = canonical.join(&id);
        let dst_skill = dst_dir.join("SKILL.md");

        if dst_skill.exists() {
            let src_rev = std::fs::read_to_string(&src_skill).map(|c| extract_revision(&c)).unwrap_or(0);
            let dst_rev = std::fs::read_to_string(&dst_skill).map(|c| extract_revision(&c)).unwrap_or(0);
            if dst_rev >= src_rev { continue; }
        }

        let _ = std::fs::create_dir_all(&dst_dir);
        if let Err(e) = copy_dir_recursive(&entry.path(), &dst_dir) {
            log::warn!("[skills] Failed to seed {:?}: {}", id, e);
        } else {
            log::info!("[skills] Seeded {:?} to canonical dir", id);
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn restart_general_agent(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let svc = (*svc).clone();
    let mux = svc.mux.clone();
    let pty = svc.pty.clone();
    let screen_state = svc.screen_state.clone();
    let config = (*config).clone();
    std::thread::spawn(move || {
        crate::service::general_agent::restart_general_agent(
            &svc, &mux, &pty, &screen_state, &config, &app,
        );
    });
    Ok(())
}

use crate::prompts::CODETERM_RULE;

#[tauri::command]
pub fn install_project_rules() -> Result<(), String> {
    let dest = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".claude")
        .join("rules");
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("codeterm.md"), CODETERM_RULE).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn check_rule_installed() -> Result<RuleStatus, String> {
    let path = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".claude")
        .join("rules")
        .join("codeterm.md");
    let bundled_revision = extract_revision(CODETERM_RULE);
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let installed_revision = extract_revision(&content);
        Ok(RuleStatus {
            installed: true,
            update_available: installed_revision < bundled_revision,
            revision: bundled_revision,
            installed_revision,
        })
    } else {
        Ok(RuleStatus {
            installed: false,
            update_available: false,
            revision: bundled_revision,
            installed_revision: 0,
        })
    }
}

#[derive(serde::Serialize)]
pub struct RuleStatus {
    pub installed: bool,
    pub update_available: bool,
    pub revision: u32,
    pub installed_revision: u32,
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let resolved = if path.starts_with("~/") || path.starts_with("~\\") {
        dirs::home_dir()
            .ok_or("Cannot find home directory")?
            .join(&path[2..])
    } else {
        std::path::PathBuf::from(&path)
    };
    if resolved.exists() {
        #[cfg(target_os = "windows")]
        { let _ = std::process::Command::new("explorer").arg(&resolved).spawn(); }
        #[cfg(target_os = "macos")]
        { let _ = std::process::Command::new("open").arg(&resolved).spawn(); }
        #[cfg(target_os = "linux")]
        { let _ = std::process::Command::new("xdg-open").arg(&resolved).spawn(); }
    }
    Ok(())
}

#[tauri::command]
pub fn test_notification(_app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args(["-e", "display notification \"Notifications are working!\" with title \"CodeTerm\""])
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = _app.notification().builder()
            .title("CodeTerm")
            .body("Notifications are working!")
            .show();
    }
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn(); }
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(&url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
    Ok(())
}

#[tauri::command]
pub fn get_autostart_enabled() -> bool {
    crate::autostart::is_enabled()
}

#[tauri::command]
pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    if enabled { crate::autostart::enable() } else { crate::autostart::disable() }
}

#[tauri::command]
pub fn list_dir_completions(cwd: String, prefix: String) -> Result<Vec<DirCompletion>, String> {
    let dir = std::path::Path::new(&cwd);
    if !dir.exists() { return Ok(vec![]); }

    let prefix_lower = prefix.to_lowercase();
    let mut results: Vec<DirCompletion> = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let ft = entry.file_type().unwrap_or_else(|_| panic!());
        if !ft.is_dir() { continue; }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }

        let name_lower = name.to_lowercase();

        let score = if name_lower.starts_with(&prefix_lower) {
            0
        } else if name_lower.contains(&prefix_lower) {
            1
        } else if subsequence_match(&name_lower, &prefix_lower) {
            2
        } else {
            continue;
        };

        results.push(DirCompletion { name, score });
    }

    results.sort_by(|a, b| a.score.cmp(&b.score).then(a.name.len().cmp(&b.name.len())));
    results.truncate(20);
    Ok(results)
}

#[derive(serde::Serialize)]
pub struct DirCompletion {
    pub name: String,
    pub score: i32,
}

fn subsequence_match(text: &str, query: &str) -> bool {
    let mut qi = query.chars().peekable();
    for tc in text.chars() {
        if qi.peek() == Some(&tc) {
            qi.next();
        }
        if qi.peek().is_none() { return true; }
    }
    qi.peek().is_none()
}

fn claude_settings_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

// v2 = hooks carry `Authorization: Bearer $CODETERM_API_TOKEN` and rely on
// auth_middleware instead of the old loopback-exemption path. Any older
// marker is treated as upgradable so we overwrite the pre-v2 curl commands.
const CODETERM_HOOK_MARKER: &str = "codeterm-hook-v2";

/// True when a hook entry was installed by CodeTerm at any version — used on
/// install to REMOVE stale entries before writing the current ones. Accepts
/// any historical marker plus a URL-based legacy fallback.
fn is_codeterm_hook(hook_entry: &serde_json::Value) -> bool {
    hook_entry.get("hooks").and_then(|hs| hs.as_array())
        .map(|hs| hs.iter().any(|h| {
            if let Some(source) = h.get("_source").and_then(|s| s.as_str()) {
                if source.starts_with("codeterm-hook") || source == "codeterm" {
                    return true;
                }
            }
            h.get("command").and_then(|c| c.as_str())
                .map(|cmd| {
                    let is_codeterm_url = cmd.contains("7685") || cmd.contains("7686");
                    let is_codeterm_path = cmd.contains("/api/notifications")
                        || cmd.contains("/api/panes/")
                        || cmd.contains("/hook/permission")
                        || cmd.contains("/hook/stop")
                        || cmd.contains("/hook/session_start")
                        || cmd.contains("/api/mem/flush");
                    is_codeterm_url && is_codeterm_path
                })
                .unwrap_or(false)
        }))
        .unwrap_or(false)
}

/// True only when the hook matches the CURRENT installer output. Drives the
/// `hooks_installed` bit so we force-upgrade stale pre-v2 entries (which
/// lacked the auth header and relied on the dropped exemption).
fn is_current_codeterm_hook(hook_entry: &serde_json::Value) -> bool {
    hook_entry.get("hooks").and_then(|hs| hs.as_array())
        .map(|hs| hs.iter().any(|h| {
            h.get("_source").and_then(|s| s.as_str())
                .map(|src| src == CODETERM_HOOK_MARKER)
                .unwrap_or(false)
        }))
        .unwrap_or(false)
}

/// Check if Claude Code hooks are installed for CodeTerm notifications.
#[tauri::command]
pub fn check_claude_hooks() -> Result<ClaudeHooksStatus, String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(ClaudeHooksStatus {
            settings_exists: false,
            hooks_installed: false,
            notification_hook: false,
            stop_hook: false,
            precompact_hook: false,
            session_start_hook: false,
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    let notification_hook = json.pointer("/hooks/Notification")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let stop_hook = json.pointer("/hooks/Stop")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let precompact_hook = json.pointer("/hooks/PreCompact")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let session_start_hook = json.pointer("/hooks/SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);

    Ok(ClaudeHooksStatus {
        settings_exists: true,
        hooks_installed: notification_hook && stop_hook && precompact_hook && session_start_hook,
        notification_hook,
        stop_hook,
        precompact_hook,
        session_start_hook,
    })
}

pub fn ensure_claude_hooks_installed() {
    match check_claude_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] claude hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_claude_hooks(None) {
                log::warn!("[startup] failed to auto-install claude hooks: {}", e);
            } else {
                log::info!("[startup] claude hooks auto-installed (Notification, Stop, PreCompact, SessionStart)");
            }
        }
    }
}

#[derive(serde::Serialize)]
pub struct ClaudeHooksStatus {
    pub settings_exists: bool,
    pub hooks_installed: bool,
    pub notification_hook: bool,
    pub stop_hook: bool,
    pub precompact_hook: bool,
    pub session_start_hook: bool,
}

#[tauri::command]
pub fn install_claude_hooks(api_port: Option<u16>) -> Result<(), String> {
    let path = claude_settings_path()?;
    let port = api_port.unwrap_or_else(|| {
        #[cfg(debug_assertions)]
        { 7686 }
        #[cfg(not(debug_assertions))]
        { 7685 }
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut json: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    let null_dev = crate::shell::ShellSyntax::NULL_DEV;
    // Hook callbacks share one auth header so the endpoints can stay behind
    // auth_middleware and don't need a loopback/LAN exemption. CodeTerm sets
    // $CODETERM_API_TOKEN in the PTY env at spawn time.
    let auth_h = "-H \"Authorization: Bearer ${CODETERM_API_TOKEN}\"";
    let notify_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' {auth_h} -d '{{}}' > {null_dev} 2>&1 || true"
    );
    let stop_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/stop -H 'Content-Type: application/json' {auth_h} -d '{{}}' > {null_dev} 2>&1 || true"
    );
    // flush PTY scrollback before summarization destroys it
    let precompact_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/mem/flush -H 'Content-Type: application/json' {auth_h} -d '{{\"pane\":\"'\"$CODETERM_PANE_ID\"'\",\"title\":\"pre-compact\"}}' > {null_dev} 2>&1 || true"
    );
    // Forward Claude's SessionStart JSON (delivered on stdin) to CodeTerm so we
    // learn the new session_id when the user runs /new, /clear, or starts fresh.
    let session_start_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' {auth_h} --data-binary @- > {null_dev} 2>&1 || true"
    );

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }

    let notif_hook = serde_json::json!({
        "matcher": "permission_prompt",
        "hooks": [{
            "type": "command",
            "command": notify_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let stop_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": stop_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let precompact_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": precompact_cmd,
            "timeout": 10000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let session_start_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": session_start_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let hooks = json["hooks"].as_object_mut()
        .ok_or("hooks is not an object")?;

    for key in &["Notification", "Stop", "PreCompact", "SessionStart"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_codeterm_hook(h));
        }
    }

    if !hooks.contains_key("Notification") {
        hooks.insert("Notification".into(), serde_json::json!([]));
    }
    hooks["Notification"].as_array_mut().unwrap().push(notif_hook);

    if !hooks.contains_key("Stop") {
        hooks.insert("Stop".into(), serde_json::json!([]));
    }
    hooks["Stop"].as_array_mut().unwrap().push(stop_hook);

    if !hooks.contains_key("PreCompact") {
        hooks.insert("PreCompact".into(), serde_json::json!([]));
    }
    hooks["PreCompact"].as_array_mut().unwrap().push(precompact_hook);

    if !hooks.contains_key("SessionStart") {
        hooks.insert("SessionStart".into(), serde_json::json!([]));
    }
    hooks["SessionStart"].as_array_mut().unwrap().push(session_start_hook);

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude Code hooks installed at {:?}", path);
    Ok(())
}

#[tauri::command]
pub fn uninstall_claude_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if !path.exists() { return Ok(()); }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["Notification", "Stop"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| !is_codeterm_hook(h));
            }
        }
    }

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude Code hooks removed");
    Ok(())
}

#[tauri::command]
pub async fn save_clipboard_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let path = crate::attachments::save_with_uuid(
        "images",
        &ext,
        &bytes,
        crate::attachments::EvictionPolicy::MaxCount(20),
    )?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn save_clipboard_file(
    bytes: Vec<u8>,
    filename: String,
) -> Result<String, String> {
    let path = crate::attachments::save_with_prefixed_name(
        "files",
        &filename,
        &bytes,
        crate::attachments::EvictionPolicy::MaxBytes(crate::attachments::DEFAULT_MAX_BYTES),
    )?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn save_temp_file(data: String, extension: String, name: Option<String>) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    let fallback = format!("codeterm-paste.{}", extension);
    let name = name.filter(|s| !s.is_empty()).unwrap_or(fallback);
    let path = crate::attachments::save_with_prefixed_name(
        "chat",
        &name,
        &bytes,
        crate::attachments::EvictionPolicy::MaxBytes(crate::attachments::DEFAULT_MAX_BYTES),
    )?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn copy_image_to_clipboard(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let escaped_path = path.replace("'", "''");
        let ps_script = format!(
            r#"
            Add-Type -AssemblyName System.Windows.Forms
            Add-Type -AssemblyName System.Drawing
            $img = [System.Drawing.Image]::FromFile('{escaped_path}')
            $data = New-Object System.Windows.Forms.DataObject
            $data.SetImage($img)
            $ms = New-Object System.IO.MemoryStream
            $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $ms.Position = 0
            $data.SetData('PNG', $false, $ms)
            [System.Windows.Forms.Clipboard]::SetDataObject($data, $true)
            $img.Dispose()
            "#
        );
        log::debug!("[clipboard] copying image to clipboard: {}", path);
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &ps_script])
            .output()
            .map_err(|e| format!("PowerShell launch failed: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("[clipboard] PowerShell failed: {}", stderr);
            return Err(format!("Clipboard copy failed: {}", stderr));
        }
        log::debug!("[clipboard] image copied to clipboard successfully");
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "set the clipboard to (read (POSIX file \"{}\") as «class PNGf»)",
            path.replace("\"", "\\\"")
        );
        let output = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            let script2 = format!(
                "set the clipboard to (read (POSIX file \"{}\") as TIFF picture)",
                path.replace("\"", "\\\"")
            );
            let output2 = std::process::Command::new("osascript")
                .args(["-e", &script2])
                .output()
                .map_err(|e| e.to_string())?;
            if !output2.status.success() {
                let stderr = String::from_utf8_lossy(&output2.stderr);
                return Err(format!("Clipboard copy failed: {}", stderr));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-target", "image/png", "-i", &path])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("xclip failed".to_string());
        }
    }

    Ok(())
}

#[tauri::command]
pub fn generate_api_token(
    state: tauri::State<'_, crate::config::ConfigState>,
    app: AppHandle,
) -> Result<String, String> {
    let token = uuid::Uuid::new_v4().to_string().replace("-", "");
    let mut cfg = state.lock().clone();
    cfg.api.token = Some(token.clone());
    crate::config::save_config_pub(&cfg)?;
    *state.lock() = cfg.clone();
    app.emit("config-changed", &cfg).ok();
    Ok(token)
}

#[tauri::command]
pub fn get_network_info(state: tauri::State<'_, crate::config::ConfigState>) -> Result<serde_json::Value, String> {
    let local_ip = crate::api::routes::local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let tailscale_ip = crate::api::routes::detect_tailscale_ip();
    let port = state.lock().api.port;
    Ok(serde_json::json!({
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "api_port": port,
        "api_url": format!("http://{}:{}", local_ip, port),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:{}", ip, port)),
    }))
}

#[tauri::command]
pub async fn get_pane_git_info(
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
) -> Result<Option<GitInfo>, String> {
    let cwd = {
        let mux = mux.lock();
        mux.get_pane(&pane_id).and_then(|p| p.cwd.clone())
    };
    let Some(cwd) = cwd else { return Ok(None) };
    Ok(tokio::task::spawn_blocking(move || git_info::get_git_info(&cwd))
        .await
        .map_err(|e| format!("join: {}", e))?)
}

/// Diagnostics surfaced in Settings → General on Windows: which shell we would
/// spawn for a new tab, and whether the faster PowerShell 7 (pwsh) is installed.
/// The Settings UI shows an install banner only when `pwsh_installed` is false.
/// Runs the filesystem walk in `spawn_blocking` because `Path::is_file()` on a
/// dead network mount in the user's PATH (SMB/VPN) can block seconds.
#[tauri::command]
pub async fn get_shell_diagnostics() -> serde_json::Value {
    let platform = std::env::consts::OS;
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(move || {
            fn exists_on_path(name: &str) -> Option<String> {
                let path = std::env::var("PATH").ok()?;
                for dir in path.split(';') {
                    if dir.is_empty() { continue; }
                    let p = std::path::Path::new(dir).join(name);
                    if p.is_file() {
                        return Some(p.to_string_lossy().into_owned());
                    }
                }
                None
            }
            let pwsh = exists_on_path("pwsh.exe");
            let ps5 = exists_on_path("powershell.exe");
            let current = pwsh.clone().or(ps5.clone())
                .unwrap_or_else(|| std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
            serde_json::json!({
                "platform": platform,
                "pwsh_installed": pwsh.is_some(),
                "pwsh_path": pwsh,
                "powershell_path": ps5,
                "current_shell": current,
            })
        })
        .await
        .unwrap_or_else(|_| serde_json::json!({
            "platform": platform,
            "pwsh_installed": true,
            "current_shell": "unknown",
        }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        serde_json::json!({
            "platform": platform,
            "pwsh_installed": true,
            "current_shell": std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()),
        })
    }
}

/// Cleanly restart the CodeTerm process. Used by the Settings UI after the user
/// installs pwsh so the new shell is picked up by `detect_default_shell`.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// Open a new shell tab and type `winget install Microsoft.PowerShell` into it,
/// without pressing Enter — the user reviews the command and hits Enter manually.
#[tauri::command]
pub fn suggest_install_pwsh(
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<String, String> {
    use crate::service::spawn::{PaneCommand, PaneVisibility, SpawnPaneSpec};
    let handle = svc.spawn_pane(SpawnPaneSpec {
        title: Some("Install pwsh 7".to_string()),
        visibility: PaneVisibility::Visible,
        command: PaneCommand::Shell,
        ..Default::default()
    })?;
    let pane_id = handle.pane_id.clone();
    let svc_arc = svc.inner().clone();
    let pane_id_bg = pane_id.clone();
    std::thread::spawn(move || {
        let cmd_text = "winget install Microsoft.PowerShell";
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            if std::time::Instant::now() > deadline {
                log::warn!("[suggest_install_pwsh] gave up waiting for PTY on {}", pane_id_bg);
                return;
            }
            let pty_id = svc_arc
                .mux
                .lock()
                .get_pane(&pane_id_bg)
                .and_then(|p| p.pty_id.clone());
            if let Some(pid) = pty_id {
                let _ = crate::service::pty_io::poll_shell_prompt(
                    &svc_arc.screen_state,
                    &pid,
                    std::time::Duration::from_secs(10),
                );
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        if let Err(e) = svc_arc.send_text(&pane_id_bg, cmd_text, false) {
            log::warn!("[suggest_install_pwsh] send_text failed: {}", e);
        }
    });
    Ok(pane_id)
}

#[tauri::command]
pub fn cron_list(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let jobs = svc.cron_list(&cron, workspace_id.as_deref());
    serde_json::to_value(jobs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_get(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let job = svc.cron_get(&cron, &id)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_create(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    name: String,
    workspace_id: String,
    schedule: String,
    task: String,
    agent: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    description: Option<String>,
    oneshot: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = crate::cron::store::CronCreateParams {
        name,
        workspace_id,
        schedule,
        task,
        description,
        agent,
        model,
        cwd,
        role: None,
        creator_pane_id: None,
        enabled: Some(true),
        max_history: None,
        oneshot,
        jitter_max_ms: None,
    };
    let job = svc.cron_create(&cron, &wake, params)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_update(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    name: Option<String>,
    schedule: Option<String>,
    task: Option<String>,
    cwd: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    agent: Option<String>,
    model: Option<String>,
    jitter_max_ms: Option<u64>,
) -> Result<serde_json::Value, String> {
    let patch = crate::cron::store::CronUpdateParams {
        name,
        description,
        schedule,
        task,
        cwd,
        role: None,
        enabled,
        max_history: None,
        agent,
        model,
        jitter_max_ms,
    };
    let job = svc.cron_update(&cron, &wake, &id, patch)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_delete(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<(), String> {
    svc.cron_delete(&cron, &wake, &id)
}

#[tauri::command]
pub fn cron_toggle(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    let job = svc.cron_enable(&cron, &wake, &id, enabled)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_trigger(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let run = svc.cron_trigger(&cron, &id)?;
    serde_json::to_value(run).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_history(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let history = svc.cron_history(&cron, &id);
    serde_json::to_value(history).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_read_log(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    job_id: String,
    run_id: String,
) -> Result<String, String> {
    if let Some(text) = cron.lock().read_log(&job_id, &run_id) {
        return Ok(text);
    }

    let pane_id = cron
        .lock()
        .get_history(&job_id)
        .iter()
        .find(|r| r.run_id == run_id)
        .and_then(|r| r.pane_id.clone());

    if let Some(pane_id) = pane_id {
        let cmd = crate::dispatch::Command::GetOutput {
            pane_id,
            offset: None,
            max_bytes: None,
            strip_ansi: true,
            full: true,
        };
        let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
        // GetOutput response uses "output", not "text"
        let text = result
            .into_json()
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Ok(text);
    }

    Err("Log not found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_extract_revision_valid() {
        assert_eq!(extract_revision("some content\n<!-- revision: 10 -->"), 10);
    }

    #[test]
    fn test_extract_revision_missing() {
        assert_eq!(extract_revision("no revision here"), 0);
    }

    #[test]
    fn test_extract_revision_whitespace() {
        assert_eq!(extract_revision("<!-- revision:   42   -->"), 42);
    }

    #[test]
    fn test_extract_revision_invalid_number() {
        assert_eq!(extract_revision("<!-- revision: abc -->"), 0);
    }

    #[test]
    fn test_hook_commands_preserve_dollar_sign() {
        // Verify that hook commands contain $CODETERM_PANE_ID (not \\$ escaped)
        let null_dev = crate::shell::ShellSyntax::NULL_DEV;
        let port = 7686u16;
        let notify_cmd = format!(
            "curl -s -X POST http://localhost:{port}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
        );
        // The command string itself must have literal $
        assert!(notify_cmd.contains("$CODETERM_PANE_ID"), "Command must contain $CODETERM_PANE_ID");
        assert!(!notify_cmd.contains("\\$"), "Command must NOT contain escaped \\$");

        // Verify serde_json does NOT escape $ in strings
        let json_val = serde_json::json!({"command": notify_cmd});
        let serialized = serde_json::to_string_pretty(&json_val).unwrap();
        assert!(serialized.contains("$CODETERM_PANE_ID"), "JSON serialization must preserve $");
        assert!(!serialized.contains("\\\\$"), "JSON serialization must NOT double-escape $");
        assert!(!serialized.contains("\\$"), "JSON serialization must NOT escape $");
    }

    #[test]
    fn test_install_hooks_to_temp_file() {
        // Test install_claude_hooks logic against a temporary settings file
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");

        // Write empty settings
        fs::write(&settings_path, "{}").unwrap();

        // Build hook commands (same logic as install_claude_hooks)
        let port = 7686u16;
        let null_dev = crate::shell::ShellSyntax::NULL_DEV;
        let notify_cmd = format!(
            "curl -s -X POST http://localhost:{port}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
        );

        let mut json: serde_json::Value = serde_json::json!({});
        json["hooks"] = serde_json::json!({});
        json["hooks"]["Notification"] = serde_json::json!([{
            "matcher": "permission_prompt",
            "hooks": [{
                "type": "command",
                "command": notify_cmd,
                "timeout": 5000,
                "_source": CODETERM_HOOK_MARKER
            }]
        }]);

        let pretty = serde_json::to_string_pretty(&json).unwrap();
        fs::write(&settings_path, &pretty).unwrap();

        // Read back and verify
        let content = fs::read_to_string(&settings_path).unwrap();
        assert!(content.contains("$CODETERM_PANE_ID"), "Written file must have $CODETERM_PANE_ID");
        assert!(!content.contains("\\$"), "Written file must NOT have escaped \\$");

        // Parse back and verify the command string
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let cmd = parsed["hooks"]["Notification"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("$CODETERM_PANE_ID"));
    }
}

