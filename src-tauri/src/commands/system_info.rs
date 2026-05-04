use tauri::{AppHandle, Emitter};

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

/// Cleanly restart the CodeTerm process. Used by the Settings UI after the user
/// installs pwsh so the new shell is picked up by `detect_default_shell`.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
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
