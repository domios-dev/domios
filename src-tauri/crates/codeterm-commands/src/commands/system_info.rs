#[tauri::command]
pub fn get_windows_build_number() -> u32 {
    // Build number is constant for the OS lifetime — cache to avoid spawning cmd.exe on every TerminalTab mount (~1s on Win11 with Defender).
    static CACHED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            let mut cmd = Command::new("cmd");
            cmd.args(["/c", "ver"]);
            codeterm_service_core::process_util::no_window_std(&mut cmd);
            if let Ok(output) = cmd.output() {
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
        {
            0
        }
    })
}

#[tauri::command]
pub fn get_default_cwd() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
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
                    if dir.is_empty() {
                        continue;
                    }
                    let p = std::path::Path::new(dir).join(name);
                    if p.is_file() {
                        return Some(p.to_string_lossy().into_owned());
                    }
                }
                None
            }
            let pwsh = exists_on_path("pwsh.exe");
            let ps5 = exists_on_path("powershell.exe");
            let current = pwsh
                .clone()
                .or(ps5.clone())
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
        .unwrap_or_else(|_| {
            serde_json::json!({
                "platform": platform,
                "pwsh_installed": true,
                "current_shell": "unknown",
            })
        })
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

/// Frontend reports WebGL renderer health here. Goes into the daemon
/// log so rendering health is observable without opening the WebView
/// devtools. Called from useXterm.ts on every TerminalTab mount.
///
/// state: "loaded" — WebGL addon attached successfully
///        "context_lost" — GPU context dropped (xterm will fall back)
///        "failed" — WebGL addon constructor threw
///        "skipped_macos" / "skipped_windows" / "skipped_test_mode" — deliberate DOM renderer
#[tauri::command]
pub fn report_webgl_status(state: String, detail: Option<String>) {
    match state.as_str() {
        "loaded" => log::info!("[webgl] addon loaded"),
        "skipped_macos" => log::info!("[webgl] skipped on macOS — DOM renderer active"),
        "skipped_windows" => log::info!("[webgl] skipped on Windows — DOM renderer active"),
        "skipped_test_mode" => log::info!("[webgl] skipped in test mode"),
        "context_lost" => log::warn!("[webgl] GPU context lost — DOM fallback engaged"),
        "failed" => log::warn!(
            "[webgl] addon failed to load: {}",
            detail.as_deref().unwrap_or("<no detail>")
        ),
        other => log::warn!("[webgl] unknown state from FE: {other}"),
    }
}
