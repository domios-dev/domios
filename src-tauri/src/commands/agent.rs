use tauri::State;

use crate::prompts::CODETERM_RULE;
use crate::service::ServiceState;

use super::skills::extract_revision;
use super::RuleStatus;

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
    let cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    let output = std::process::Command::new(cmd).arg(&binary).output();
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
