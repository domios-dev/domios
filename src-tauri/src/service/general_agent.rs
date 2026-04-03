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

/// Generate or update the provider instructions file (e.g., CLAUDE.md).
/// Uses marker-based sections — auto-generated content between markers is replaced,
/// user content outside markers is preserved.
pub fn generate_instructions(provider_name: &str, instructions_filename: &str) {
    let path = general_dir().join(instructions_filename);
    let version = env!("CARGO_PKG_VERSION");
    let platform = if cfg!(target_os = "windows") { "Windows" }
        else if cfg!(target_os = "macos") { "macOS" }
        else { "Linux" };
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let auto_section = crate::prompts::GENERAL_AGENT_TEMPLATE
        .replace("{version}", version)
        .replace("{platform}", platform)
        .replace("{hostname}", &hostname)
        .replace("{provider_name}", provider_name)
        .replace("{general_dir}", &general_dir().to_string_lossy())
        .replace("{secrets_dir}", &secrets_dir().to_string_lossy());

    // Read existing file to preserve user content outside markers
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let new_content = if existing.contains("<!-- CODETERM-AUTO-START") && existing.contains("<!-- CODETERM-AUTO-END -->") {
        // Replace content between markers
        let start_marker = "<!-- CODETERM-AUTO-START";
        let end_marker = "<!-- CODETERM-AUTO-END -->";
        let start = existing.find(start_marker).unwrap();
        let end = existing.find(end_marker).unwrap() + end_marker.len();
        let after = existing[end..].trim_start_matches('\n');
        format!("{}{}\n\n{}", &existing[..start], auto_section.trim_end(), after)
    } else if existing.is_empty() {
        // Fresh file — add auto section + pointer to extra instructions
        format!("{}\n", auto_section)
    } else {
        // File exists but no markers — prepend auto section, keep everything
        format!("{}\n\n{}", auto_section, existing)
    };

    if let Err(e) = std::fs::write(&path, new_content) {
        log::error!("[general-agent] Failed to write instructions file {:?}: {}", path, e);
    } else {
        log::info!("[general-agent] Instructions file updated: {:?}", path);
    }

    // Write bootstrap.md on first run — agent reads it, completes onboarding, then deletes it
    let bootstrap = general_dir().join("bootstrap.md");
    if !bootstrap.exists() {
        let _ = std::fs::write(&bootstrap, crate::prompts::BOOTSTRAP_INSTRUCTIONS_TEMPLATE);
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
    app: &tauri::AppHandle,
) {
    let general_config = config.lock().general_agent.clone();
    let provider_name = &general_config.agent_provider;

    log::info!("[general-agent] Starting (provider={})", provider_name);

    let general_cwd = general_dir().to_string_lossy().to_string();

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
                let sid = m.get_pane(&pid).and_then(|p| p.agent_session_id.clone());
                (pid, sid)
            }
            None => {
                log::error!("[general-agent] No General pane found — ensure_general_workspace failed?");
                return;
            }
        }
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
    mux.lock().link_pty(&pane_id, pty_id.clone());
    // Set agent type immediately
    svc.set_pane_agent_type(&pane_id, Some(provider_name.to_string()));
    log::info!("[general-agent] PTY {} linked to pane {}", pty_id, pane_id);

    // Wait for shell prompt before sending launch command
    std::thread::sleep(std::time::Duration::from_secs(1));

    // Build launch command
    let cmd = if let Some(ref sid) = session_id {
        // Resume
        svc.agent_registry.get(provider_name)
            .and_then(|p| p.build_resume_command(sid, true))
            .unwrap_or_else(|| format!("claude --resume {} --dangerously-skip-permissions", sid))
    } else {
        // Fresh start
        svc.agent_registry.get(provider_name)
            .map(|p| {
                let params = crate::agent::provider::AgentLaunchParams {
                    agent_name: None,
                    task: None,
                    args: None,
                    skip_permissions: true,
                };
                p.build_launch_command(&params)
            })
            .unwrap_or_else(|| "claude --dangerously-skip-permissions".to_string())
    };

    log::info!("[general-agent] Launching: {}", cmd);
    // Use bracket paste to prevent shell from eating first character
    let paste = format!("\x1b[200~{}\x1b[201~\r", cmd);
    if let Err(e) = pty.lock().write(&pty_id, paste.as_bytes()) {
        log::error!("[general-agent] Failed to write launch command: {}", e);
    }

    // Emit state changed so frontend picks up the new workspace/pane
    svc.emit_state_changed();
    log::info!("[general-agent] Started successfully (pane={}, pty={})", pane_id, pty_id);
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
    fn test_generate_instructions_fresh() {
        let tmp = std::env::temp_dir().join("codeterm-test-general");
        let _ = std::fs::create_dir_all(&tmp);
        let path = tmp.join("CLAUDE.md");
        let _ = std::fs::remove_file(&path);

        // Can't easily test generate_instructions directly (uses general_dir()),
        // but we can test the marker logic
        let auto_start = "<!-- CODETERM-AUTO-START v0.1.0 -->\nAuto content\n<!-- CODETERM-AUTO-END -->";
        let full = format!("{}\n\nUser content here", auto_start);

        // Simulate update: replace auto section
        let new_auto = "<!-- CODETERM-AUTO-START v0.2.0 -->\nNew auto content\n<!-- CODETERM-AUTO-END -->";
        let start_marker = "<!-- CODETERM-AUTO-START";
        let end_marker = "<!-- CODETERM-AUTO-END -->";
        let start = full.find(start_marker).unwrap();
        let end = full.find(end_marker).unwrap() + end_marker.len();
        let result = format!("{}{}{}", &full[..start], new_auto, &full[end..]);

        assert!(result.contains("v0.2.0"));
        assert!(result.contains("User content here"));
        assert!(!result.contains("v0.1.0"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
