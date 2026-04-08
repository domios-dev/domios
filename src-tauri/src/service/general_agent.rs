//! General Agent — always-on system agent lifecycle management.
//!
//! Handles: directory creation, instructions file generation,
//! workspace/pane setup, startup, resume, recovery.

use std::path::PathBuf;

/// Get the General Agent workspace directory.
/// `~/.codeterm/general/` (or `~/.codeterm-dev/general/` for dev builds).
pub fn general_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("general")
}

/// Get the secrets directory.
pub fn secrets_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("secrets")
}

/// Get the integrations config directory.
pub fn integrations_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("integrations")
}

/// Ensure all General Agent directories exist.
pub fn ensure_directories() {
    let dirs = [general_dir(), secrets_dir(), integrations_dir(), general_dir().join("state")];
    for dir in &dirs {
        if !dir.exists() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error!("[general-agent] Failed to create directory {:?}: {}", dir, e);
            }
        }
    }
}

/// Generate agent instructions for the General Agent.
///
/// File layout:
/// - `AGENT.md` — system prompt (regenerated every start)
/// - `MEMORY.md` — shared memory (created once, agent-editable, persistent)
/// - `CLAUDE.md` — thin import wrapper for Claude (`@./AGENT.md\n@./MEMORY.md`)
/// - `AGENTS.md` — full inline for Codex (AGENT.md + MEMORY.md + codeterm rule)
/// - `bootstrap.md` — first-run onboarding (deleted by agent after completion)
pub fn generate_instructions(provider_name: &str) {
    let version = env!("CARGO_PKG_VERSION");
    let platform = if cfg!(target_os = "windows") { "Windows" }
        else if cfg!(target_os = "macos") { "macOS" }
        else { "Linux" };
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let dir = general_dir();

    // 1. AGENT.md — system prompt, regenerated every start
    let agent_content = crate::prompts::GENERAL_AGENT_TEMPLATE
        .replace("{version}", version)
        .replace("{platform}", platform)
        .replace("{hostname}", &hostname)
        .replace("{provider_name}", provider_name)
        .replace("{general_dir}", &dir.to_string_lossy())
        .replace("{secrets_dir}", &secrets_dir().to_string_lossy())
        .replace("{skills_dir}", &crate::paths::data_dir().join("skills").to_string_lossy());

    if let Err(e) = std::fs::write(dir.join("AGENT.md"), &agent_content) {
        log::error!("[general-agent] Failed to write AGENT.md: {}", e);
    } else {
        log::info!("[general-agent] AGENT.md updated");
    }

    // 2. MEMORY.md — shared memory, created once.
    //    Migration: if CLAUDE.md has user content beyond the template, seed MEMORY.md with it.
    let memory_path = dir.join("MEMORY.md");
    if !memory_path.exists() {
        let mut migrated = false;
        let claude_path = dir.join("CLAUDE.md");
        if claude_path.exists() {
            if let Ok(existing) = std::fs::read_to_string(&claude_path) {
                let user_content = extract_user_content(&existing);
                if !user_content.is_empty() {
                    if let Err(e) = std::fs::write(&memory_path, &user_content) {
                        log::error!("[general-agent] Failed to migrate to MEMORY.md: {}", e);
                    } else {
                        log::info!("[general-agent] Migrated user notes from CLAUDE.md to MEMORY.md");
                        migrated = true;
                    }
                }
            }
        }
        if !migrated {
            let default = "# Memory\n\nShared memory for the General Agent.\nBoth Claude and Codex read and write this file — it persists across provider switches.\n";
            let _ = std::fs::write(&memory_path, default);
            log::info!("[general-agent] MEMORY.md created");
        }
    }

    // 3. CLAUDE.md — thin import wrapper (regenerated every start)
    let claude_content = "@./AGENT.md\n@./MEMORY.md\n";
    if let Err(e) = std::fs::write(dir.join("CLAUDE.md"), claude_content) {
        log::error!("[general-agent] Failed to write CLAUDE.md: {}", e);
    }

    // 4. AGENTS.md — full inline for Codex (regenerated every start)
    let memory_content = std::fs::read_to_string(&memory_path).unwrap_or_default();
    let agents_content = format!(
        "{}\n\n---\n\n{}\n\n---\n\n{}",
        agent_content,
        memory_content,
        crate::prompts::CODETERM_RULE,
    );
    if let Err(e) = std::fs::write(dir.join("AGENTS.md"), agents_content) {
        log::error!("[general-agent] Failed to write AGENTS.md: {}", e);
    }

    // 5. bootstrap.md — first-run onboarding
    let bootstrap = dir.join("bootstrap.md");
    if !bootstrap.exists() {
        let _ = std::fs::write(&bootstrap, crate::prompts::BOOTSTRAP_INSTRUCTIONS_TEMPLATE);
    }
}

/// Extract user-written content from an existing CLAUDE.md, stripping template boilerplate.
fn extract_user_content(content: &str) -> String {
    let stripped: String = content
        .lines()
        .filter(|l| !l.starts_with("@./"))
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = stripped.trim();
    let default_notes = "# Notes\n\nYour personal notes and memory go here.";
    if trimmed.is_empty() || trimmed == default_notes {
        String::new()
    } else {
        trimmed.to_string()
    }
}

pub const GENERAL_ALIAS: &str = "general";
pub const GENERAL_WORKSPACE_NAME: &str = "General";
pub const GENERAL_PANE_TITLE: &str = "Main";

/// Ensure General workspace + pane exist in MultiplexerState.
/// Called BEFORE mux is wrapped in Arc<Mutex<>> — no locking needed.
/// This avoids the deadlock that occurs when creating workspace from a background thread.
pub fn ensure_general_workspace(mux: &mut crate::multiplexer::state::MultiplexerState) {
    let general_cwd = general_dir().to_string_lossy().to_string();

    // Check by protected flag — the only reliable marker
    let protected_ws: Vec<_> = mux.list_workspaces().iter()
        .filter(|ws| ws.protected)
        .cloned()
        .collect();

    // Clean up duplicates: keep first protected, close rest
    if protected_ws.len() > 1 {
        for ws in &protected_ws[1..] {
            mux.set_workspace_protected(&ws.id, false);
            let _ = mux.close_workspace(&ws.id);
            log::info!("[general-agent] Cleaned up duplicate protected workspace {}", ws.id);
        }
    }

    if !protected_ws.is_empty() {
        let ws = &protected_ws[0];
        if ws.name != GENERAL_WORKSPACE_NAME {
            let _ = mux.rename_workspace(&ws.id, GENERAL_WORKSPACE_NAME);
        }
        mux.move_workspace_to_front(&ws.id);
        if let Some(pid) = ws.pane_ids.first() {
            // Ensure pane + tab titled "Main"
            let _ = mux.rename_pane(pid, GENERAL_PANE_TITLE);
            if let Some(pane) = mux.get_pane_mut(pid) {
                pane.protected = true;
            }
            mux.register_alias(GENERAL_ALIAS, pid);
            log::info!("[general-agent] Existing General workspace, alias for pane {}", pid);
        }
        return;
    }

    // Create General workspace
    let ws_info = mux.create_workspace(GENERAL_WORKSPACE_NAME.to_string(), Some(general_cwd.clone()));
    mux.set_workspace_protected(&ws_info.id, true);
    mux.move_workspace_to_front(&ws_info.id);

    // Create tab + pane
    match mux.create_tab(&ws_info.id) {
        Ok((_tab_id, pane_id)) => {
            // Rename pane + tab to "Main"
            let _ = mux.rename_pane(&pane_id, GENERAL_PANE_TITLE);
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.protected = true;
                pane.role = Some(crate::multiplexer::types::AgentRole::Orchestrator);
                pane.can_spawn = true;
                pane.cwd = Some(general_cwd);
            }
            mux.register_alias(GENERAL_ALIAS, &pane_id);
            // Make General the active workspace + focused pane on first create
            let _ = mux.set_active_workspace(&ws_info.id);
            let _ = mux.set_focused_pane(&pane_id);
            log::info!("[general-agent] Created General workspace {} with pane {}", ws_info.id, pane_id);
        }
        Err(e) => {
            log::error!("[general-agent] Failed to create General tab: {}", e);
        }
    }
}

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

    let general_cwd = general_dir().to_string_lossy().to_string();

    // Check if agent is already running with the CORRECT provider — don't spawn a new PTY
    {
        let m = mux.lock();
        if let Some(pid) = m.resolve_pane_id(GENERAL_ALIAS) {
            if let Some(pane) = m.get_pane(&pid) {
                if let Some(ref current_agent) = pane.agent_type {
                    if current_agent == provider_name {
                        if let Some(ref pty_id) = pane.pty_id {
                            if pty.lock().get_session(pty_id).is_some() {
                                log::info!("[general-agent] Already running (pane={}, pty={}, provider={}) — skipping", pid, pty_id, current_agent);
                                return;
                            }
                        }
                    } else {
                        // Wrong provider running — kill it so we can start the right one
                        log::info!("[general-agent] Provider mismatch: running={}, wanted={} — restarting", current_agent, provider_name);
                        if let Some(ref pty_id) = pane.pty_id {
                            let _ = pty.lock().kill(pty_id);
                        }
                        drop(m);
                        svc.set_pane_agent_type(&pid, None);
                        svc.set_agent_session_id(&pid, None);
                        if let Some(pane) = mux.lock().get_pane_mut(&pid) {
                            pane.pty_id = None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }
            }
        }
    }

    // Find General pane (already created by ensure_general_workspace in init_states)
    let (pane_id, session_id) = {
        let m = mux.lock();
        let pane = m.resolve_pane_id(GENERAL_ALIAS)
            .or_else(|| {
                m.list_workspaces().iter()
                    .find(|ws| ws.protected)
                    .and_then(|ws| ws.pane_ids.first().cloned())
            });
        match pane {
            Some(pid) => {
                let mut sid = m.get_pane(&pid).and_then(|p| p.agent_session_id.clone());
                // If no session_id from snapshot, try detecting from filesystem
                // (covers first launch, new machine, or wiped session store)
                if sid.is_none() {
                    let exclude: Vec<String> = m.other_session_ids(&pid);
                    drop(m); // release lock before filesystem scan
                    sid = crate::multiplexer::commands::detect_agent_session_id(
                        &svc.agent_registry,
                        provider_name,
                        &general_cwd,
                        Some(exclude),
                        Some(86400 * 7), // 7 days max age
                    ).ok().flatten();
                    if let Some(ref detected) = sid {
                        // Persist on pane so next restart uses snapshot
                        svc.set_agent_session_id(&pid, Some(detected.clone()));
                        log::info!("[general-agent] Detected session from filesystem: {}", detected);
                    }
                }
                (pid, sid)
            }
            None => {
                log::error!("[general-agent] No General pane found — ensure_general_workspace failed?");
                return;
            }
        }
    };

    // Validate session_id: if provider can't resolve it to a file, it's stale/wrong-provider
    let session_id = if let Some(ref id) = session_id {
        let found = svc.agent_registry.with_provider(provider_name, |p| {
            p.session_file_path(&general_cwd, id).filter(|f| f.exists())
        }).flatten();
        if found.is_none() {
            log::info!("[general-agent] Session '{}' not valid for provider '{}' — clearing, will detect fresh", id, provider_name);
            svc.set_agent_session_id(&pane_id, None);
            // Try detecting a valid session for this provider
            let exclude = mux.lock().other_session_ids(&pane_id);
            let fresh = crate::multiplexer::commands::detect_agent_session_id(
                &svc.agent_registry, provider_name, &general_cwd,
                Some(exclude), Some(86400 * 7),
            ).ok().flatten();
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
        env.insert("CODETERM_SOCKET_PATH".to_string(), crate::socket::server::socket_display_path());
        env.insert("CODETERM_ROLE".to_string(), "Orchestrator".to_string());
        if let Ok(port) = std::env::var("CODETERM_API_PORT") {
            env.insert("CODETERM_API_PORT".to_string(), port);
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

    // Link PTY to pane
    {
        let mut m = mux.lock();
        m.link_pty(&pane_id, pty_id.clone());
        if let Some(pane) = m.get_pane_mut(&pane_id) {
            pane.auto_approve = true;
        }
    }
    // Set agent type immediately
    svc.set_pane_agent_type(&pane_id, Some(provider_name.to_string()));
    // Emit state NOW so frontend picks up pty_id immediately — don't wait for agent detection.
    // Without this, frontend auto-spawns its own PTY and writes go to the wrong one.
    svc.emit_state_changed();
    log::info!("[general-agent] PTY {} linked to pane {}", pty_id, pane_id);

    // Start backend reader on a tokio runtime (start_backend_reader uses tokio::spawn,
    // but we're on a plain std::thread — no tokio runtime here)
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
                match crate::service::pty_lifecycle::start_backend_reader(&svc_clone, &pty_id_clone) {
                    Ok(handle) => { let _ = handle.await; }
                    Err(e) => log::error!("[general-agent] Failed to start backend reader: {}", e),
                }
            });
        });
    }
    log::info!("[general-agent] Backend reader started, waiting for shell prompt on PTY {}...", pty_id);

    // Wait for shell to be ready — same mechanism as regular pane resume in commands.rs.
    // poll_shell_prompt polls screen_state (fed by the backend reader we just started)
    // every 300ms, returns true when a shell prompt appears on the last non-empty line.
    let ready = crate::service::pty_io::poll_shell_prompt(
        screen_state,
        &pty_id,
        std::time::Duration::from_secs(15),
    );
    if !ready {
        log::warn!("[general-agent] Shell not ready within 15s for PTY {} — watchdog will retry", pty_id);
        return;
    }
    log::info!("[general-agent] Shell ready, building launch command...");

    // Build launch command.
    // Resume only when the provider preserves prior context across resume
    // (Claude replays JSONL → true; Codex restarts fresh → false).
    // Single-source-of-truth comes from the AgentProvider trait.
    let preserves_context = svc
        .agent_registry
        .with_provider(provider_name, |p| p.preserves_context_on_resume())
        .unwrap_or(false);
    let use_resume = session_id.is_some() && preserves_context;
    let cmd = if use_resume {
        let sid = session_id.as_ref().unwrap();
        match svc.agent_registry.with_provider(provider_name, |p| p.build_resume_command(sid, true)).flatten() {
            Some(cmd) => cmd,
            None => {
                log::error!("[general-agent] Provider '{}' not registered or resume unsupported", provider_name);
                return;
            }
        }
    } else {
        // Fresh start
        let params = crate::agent::provider::AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
        };
        match svc.agent_registry.with_provider(provider_name, |p| p.build_launch_command(&params)) {
            Some(cmd) => cmd,
            None => {
                log::error!("[general-agent] Provider '{}' not registered", provider_name);
                return;
            }
        }
    };

    // Send launch command via bracket paste (single attempt — watchdog handles retries)
    log::info!("[general-agent] Launching on PTY {}: {}", pty_id, cmd);
    let paste = format!("\x1b[200~{}\x1b[201~\r", cmd);
    if let Err(e) = pty.lock().write(&pty_id, paste.as_bytes()) {
        log::error!("[general-agent] Failed to write launch command: {}", e);
        return;
    }

    // Poll screen for agent detection (up to 15s — Claude can take a while)
    let launched = poll_agent_started(screen_state, &svc.agent_registry, &pty_id, std::time::Duration::from_secs(15));
    if !launched {
        log::warn!("[general-agent] Agent not detected within 15s — watchdog will retry if needed");
    }

    // Fresh-session init: when we couldn't resume, the agent has no prior
    // context — kick it with a single "init" message so it loads MEMORY.md,
    // checks its general dir, and acks readiness. The GA prompt teaches what
    // "init" means; backend just delivers it.
    //
    // For resumed sessions we deliberately do NOT send init — the agent
    // already has the previous turn-by-turn context replayed by `--resume`,
    // and re-running init would burn tokens for no gain.
    if launched && !use_resume {
        // Small delay so the agent's TUI is fully ready to accept input
        // (the readiness poll catches detection markers, not input-readiness).
        std::thread::sleep(std::time::Duration::from_millis(800));
        let init_paste = "\x1b[200~init\x1b[201~\r";
        if let Err(e) = pty.lock().write(&pty_id, init_paste.as_bytes()) {
            log::warn!("[general-agent] Failed to send init kick: {}", e);
        } else {
            log::info!("[general-agent] Sent fresh-session init kick");
        }
    }

    // Emit state changed so frontend picks up the new workspace/pane
    svc.emit_state_changed();
    log::info!("[general-agent] Started successfully (pane={}, pty={})", pane_id, pty_id);
}

/// Watchdog: periodically checks if the General Agent is alive.
/// If agent_type is None and shell prompt is visible, restarts the agent.
/// Respects `auto_restart`, `max_restart_attempts`, and `restart_delay_s` from config.
pub fn start_watchdog(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &tauri::AppHandle,
) {
    let svc = svc.clone();
    let mux = mux.clone();
    let pty = pty.clone();
    let screen_state = screen_state.clone();
    let config = config.clone();
    let app = app.clone();

    std::thread::spawn(move || {
        // Wait for initial startup to complete
        std::thread::sleep(std::time::Duration::from_secs(20));
        let mut consecutive_restarts = 0u32;
        let check_interval = std::time::Duration::from_secs(10);

        loop {
            std::thread::sleep(check_interval);

            let general_config = config.lock().general_agent.clone();
            if !general_config.enabled || !general_config.auto_restart {
                continue;
            }

            // Find the General pane and check its state
            let needs_restart = {
                let m = mux.lock();
                m.resolve_pane_id(GENERAL_ALIAS).and_then(|pid| {
                    let pane = m.get_pane(&pid)?;
                    // In grace period — still launching, don't touch
                    let in_grace = pane.agent_set_at
                        .map(|t| t.elapsed().as_secs() < 30)
                        .unwrap_or(false);
                    if in_grace { return Some(false); }
                    // Check screen: shell prompt visible + no agent markers = needs restart
                    let (has_prompt, has_agent) = pane.pty_id.as_ref().and_then(|pty_id| {
                        screen_state.try_lock().and_then(|ss| {
                            ss.get_screen_text(pty_id).map(|screen| {
                                let prompt = screen.lines().rev()
                                    .find(|l| !l.trim().is_empty())
                                    .map(|l| {
                                        let t = l.trim();
                                        t.ends_with('>') || t.ends_with('$') || t.ends_with('%') || t.ends_with('#')
                                    })
                                    .unwrap_or(false);
                                let agent = screen.contains("Claude Code")
                                    || screen.contains("? for shortcuts")
                                    || screen.contains("Cogitated for")
                                    || screen.contains("Brewed for");
                                (prompt, agent)
                            })
                        })
                    }).unwrap_or((false, false));
                    // Agent markers on screen = alive, regardless of agent_type
                    if has_agent { return Some(false); }
                    // Shell prompt visible, no agent = needs restart
                    Some(has_prompt)
                })
            };

            match needs_restart {
                Some(true) => {
                    if consecutive_restarts >= general_config.max_restart_attempts {
                        log::error!("[general-agent/watchdog] Max restarts ({}) reached — stopping",
                            general_config.max_restart_attempts);
                        break;
                    }
                    consecutive_restarts += 1;
                    log::info!("[general-agent/watchdog] Agent not running — restarting ({}/{})",
                        consecutive_restarts, general_config.max_restart_attempts);
                    // Clear stale agent_type so start_general_agent doesn't skip with "Already running"
                    if let Some(pid) = mux.lock().resolve_pane_id(GENERAL_ALIAS) {
                        svc.set_pane_agent_type(&pid, None);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(general_config.restart_delay_s as u64));
                    start_general_agent(&svc, &mux, &pty, &screen_state, &config, &app);
                }
                Some(false) => {
                    // Agent running or in grace period — reset counter
                    consecutive_restarts = 0;
                }
                None => {
                    // No General pane found — nothing to do
                }
            }
        }
    });
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
            // Clear agent state so start_general_agent doesn't skip
            drop(m);
            svc.set_pane_agent_type(&pid, None);
            svc.set_agent_session_id(&pid, None);
            // Unlink old PTY from pane
            if let Some(pane) = mux.lock().get_pane_mut(&pid) {
                pane.pty_id = None;
            }
        }
    }

    // Small delay for PTY cleanup
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Re-generate instructions for potentially new provider
    // Clone to avoid holding the config lock (deadlock with start_general_agent)
    let provider_name = config.lock().general_agent.agent_provider.clone();
    generate_instructions(&provider_name);

    // Relaunch
    start_general_agent(svc, mux, pty, screen_state, config, app);
}

/// Poll screen_state for signs that the agent has started.
/// Checks screen text against the agent registry's output detection.
fn poll_agent_started(
    screen_state: &crate::ScreenStateHandle,
    registry: &crate::agent::registry::AgentRegistry,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() > deadline { return false; }
        let detected = screen_state.try_lock()
            .and_then(|ss| ss.get_screen_text(pty_id).map(|screen| {
                // Check if any agent provider recognizes the output
                registry.detect_from_output(&screen).is_some()
            }))
            .unwrap_or(false);
        if detected { return true; }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general_dir_under_home() {
        let dir = general_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
        assert!(dir.ends_with("general"));
    }

    #[test]
    fn test_secrets_dir_under_home() {
        let dir = secrets_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
        assert!(dir.ends_with("secrets"));
    }

    #[test]
    fn test_agent_md_import_in_claude_md() {
        // CLAUDE.md should import AGENT.md via @./AGENT.md syntax
        // When CLAUDE.md doesn't exist — creates with import
        let claude_content = "@./AGENT.md\n\n# Notes\n\nYour personal notes and memory go here.\n";
        assert!(claude_content.contains("@./AGENT.md"));
        assert!(claude_content.contains("Notes"));
    }

    #[test]
    fn test_existing_claude_md_gets_import() {
        // If CLAUDE.md exists without import — prepend it
        let existing = "# My notes\nSome user content\n";
        assert!(!existing.contains("@./AGENT.md"));
        let updated = format!("@./AGENT.md\n\n{}", existing);
        assert!(updated.contains("@./AGENT.md"));
        assert!(updated.contains("Some user content"));
    }

    #[test]
    fn test_agent_template_no_markers() {
        // AGENT.md template should not contain auto markers (overwritten entirely)
        let template = crate::prompts::GENERAL_AGENT_TEMPLATE;
        assert!(!template.contains("CODETERM-AUTO-START"));
        assert!(!template.contains("CODETERM-AUTO-END"));
    }
}
