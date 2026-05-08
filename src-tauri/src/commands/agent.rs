use tauri::State;

use crate::prompts::CODETERM_RULE;
use crate::service::ServiceState;

use super::skills::extract_revision;
use super::RuleStatus;

/// Result of `install_provider_in_pane` — the new tab/pane the install
/// command was queued in. Frontend uses these to switch focus.
#[derive(serde::Serialize)]
pub struct InstallPaneResult {
    pub tab_id: String,
    pub pane_id: String,
}

/// Spawn a fresh tab in the active workspace and queue the provider's
/// install command into it. The install command comes from the provider
/// plugin's `install_command(platform)` — opencode would receive
/// `npm install -g opencode-ai`, claude `npm install -g @anthropic-ai/claude-code`,
/// etc. The Install card on the per-provider settings screen calls this.
#[tauri::command]
pub fn install_provider_in_pane(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<InstallPaneResult, String> {
    // 1. Get install command from provider plugin (platform-specific).
    let platform = crate::agent::provider::current_platform_tag();
    let install_cmd = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.install_command(platform))
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
        .ok_or_else(|| format!("Provider '{}' has no install command for {}", provider_id, platform))?;

    // 2. Resolve workspace — prefer active, fall back to first.
    let ws_id = {
        let mux = svc.mux.lock();
        mux.snapshot()
            .active_workspace_id
            .clone()
            .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
            .ok_or("No workspace found")?
    };

    // 3. Create tab + activate it so the user sees the install run.
    let (tab_id, pane_id) = svc.create_tab(&ws_id)?;

    // 4. Name the pane "Install <provider>" at the User level so auto-rename
    //    doesn't overwrite it once npm/brew prints output.
    let _ = svc.rename_pane(&pane_id, &format!("Install {}", provider_id));

    // 5. Background: wait for the PTY to land (frontend creates it on
    //    mux-state-changed), wait for shell prompt readiness, then send
    //    the install command. Mirrors `launch_agent_in_background`.
    let svc_clone = (*svc).clone();
    let pane_id_clone = pane_id.clone();
    std::thread::spawn(move || {
        let frontend_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pty_id = loop {
            if let Some(pid) = svc_clone
                .mux
                .try_lock()
                .and_then(|m| m.get_pane(&pane_id_clone).and_then(|p| p.pty_id.clone()))
            {
                break pid;
            }
            if std::time::Instant::now() > frontend_deadline {
                log::warn!("install_provider_in_pane: PTY didn't appear for pane {}", pane_id_clone);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        };
        crate::service::pty_io::poll_shell_prompt(
            &svc_clone.screen_state,
            &pty_id,
            std::time::Duration::from_secs(10),
        );
        if let Err(e) = svc_clone.send_shell_spawn(&pty_id, &install_cmd) {
            log::error!("install_provider_in_pane: failed to send cmd: {}", e);
        }
    });

    Ok(InstallPaneResult { tab_id, pane_id })
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

/// Probe PATH for a CLI binary. Used by the per-provider Installation card
/// in Settings → Agents to render the install/missing badge. Returns
/// `Ok(true)` when the binary is found, `Ok(false)` when missing — never
/// errors. Frontend treats either result as authoritative; transient PATH
/// changes are handled by re-fetching when the user reopens the panel.
#[tauri::command]
pub fn check_cli_installed(binary: String) -> Result<bool, String> {
    if binary.is_empty() {
        return Ok(false);
    }
    // Reject anything that looks like a shell metacharacter — `binary` comes
    // from a frontend-supplied provider id and we don't want it interpreted
    // as a path traversal or compound command.
    if binary.chars().any(|c| matches!(c, '/' | '\\' | ' ' | ';' | '|' | '&' | '$' | '`' | '<' | '>' | '\n')) {
        return Ok(false);
    }
    let cmd_name = if cfg!(target_os = "windows") { "where" } else { "which" };
    let mut cmd = std::process::Command::new(cmd_name);
    cmd.arg(&binary);
    crate::process_util::no_window_std(&mut cmd);
    let output = cmd.output();
    match output {
        Ok(out) if out.status.success() && !out.stdout.is_empty() => Ok(true),
        Ok(_) => Ok(false),
        // PATH probe failed at the OS level (e.g. `which` itself missing) —
        // surface as "not installed" so the UI offers Install rather than
        // hanging on an error toast.
        Err(_) => Ok(false),
    }
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
