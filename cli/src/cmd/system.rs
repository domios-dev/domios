//! `codeterm system` — OS-level autostart triggers (schtasks / launchd /
//! systemd). Explicit user invocation only; never auto-enabled.

use crate::SystemAction;

pub fn handle_system(json_mode: bool, action: SystemAction) {
    let result: Result<serde_json::Value, String> = match action {
        SystemAction::EnableAutostart { binary } => enable_autostart(binary.as_deref()),
        SystemAction::DisableAutostart => disable_autostart(),
        SystemAction::AutostartStatus => autostart_status(),
    };
    crate::cmd::print::handle_result(json_mode, result);
}

fn resolve_app_binary(override_path: Option<&str>) -> Result<std::path::PathBuf, String> {
    if let Some(p) = override_path {
        let path = std::path::PathBuf::from(p);
        if !path.exists() {
            return Err(format!("override binary not found: {}", path.display()));
        }
        return Ok(path);
    }
    default_app_binary()
}

#[cfg(target_os = "windows")]
fn default_app_binary() -> Result<std::path::PathBuf, String> {
    let candidates = [
        std::env::var_os("LOCALAPPDATA").map(|p| {
            std::path::PathBuf::from(p)
                .join("Programs")
                .join("codeterm")
                .join("CodeTerm.exe")
        }),
        std::env::var_os("ProgramFiles").map(|p| {
            std::path::PathBuf::from(p)
                .join("CodeTerm")
                .join("CodeTerm.exe")
        }),
    ];
    for c in candidates.iter().flatten() {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err("CodeTerm.exe not found in LOCALAPPDATA\\Programs\\codeterm or ProgramFiles\\CodeTerm — pass --binary to override".into())
}

#[cfg(target_os = "macos")]
fn default_app_binary() -> Result<std::path::PathBuf, String> {
    let candidates = [
        std::path::PathBuf::from("/Applications/CodeTerm.app/Contents/MacOS/CodeTerm"),
        dirs::home_dir()
            .map(|h| h.join("Applications/CodeTerm.app/Contents/MacOS/CodeTerm"))
            .unwrap_or_default(),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(
        "CodeTerm.app not found in /Applications or ~/Applications — pass --binary to override"
            .into(),
    )
}

#[cfg(target_os = "linux")]
fn default_app_binary() -> Result<std::path::PathBuf, String> {
    let candidates = [
        std::path::PathBuf::from("/usr/local/bin/codeterm"),
        std::path::PathBuf::from("/usr/bin/codeterm"),
        dirs::home_dir()
            .map(|h| h.join(".local/bin/codeterm"))
            .unwrap_or_default(),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err("codeterm binary not found in /usr/local/bin, /usr/bin, or ~/.local/bin — pass --binary to override".into())
}

// ────────────────────────── Windows ──────────────────────────

#[cfg(target_os = "windows")]
const WIN_TASK_NAME: &str = "CodeTerm-Autostart";

#[cfg(target_os = "windows")]
fn enable_autostart(binary_override: Option<&str>) -> Result<serde_json::Value, String> {
    let bin = resolve_app_binary(binary_override)?;
    let bin_str = bin.to_string_lossy().to_string();

    // PowerShell wrapper: only launches if codeterm.exe isn't already running. Drives both the ONLOGON trigger and the every-5-minute repeat.
    let ps_command = format!(
        "if (-not (Get-Process -Name CodeTerm -ErrorAction SilentlyContinue)) {{ Start-Process -FilePath '{}' }}",
        bin_str.replace('\'', "''")
    );
    let tr = format!(
        "powershell.exe -NoProfile -WindowStyle Hidden -Command \"{}\"",
        ps_command
    );

    let _ = run_schtasks(&["/Delete", "/TN", WIN_TASK_NAME, "/F"]);
    run_schtasks(&[
        "/Create",
        "/TN",
        WIN_TASK_NAME,
        "/TR",
        &tr,
        "/SC",
        "MINUTE",
        "/MO",
        "5",
        "/F",
        "/RL",
        "LIMITED",
    ])?;

    // schtasks accepts one trigger per /Create; second task gives instant logon launch while the 5-minute task covers post-logon crashes.
    let logon_name = format!("{}-Logon", WIN_TASK_NAME);
    let _ = run_schtasks(&["/Delete", "/TN", &logon_name, "/F"]);
    let _ = run_schtasks(&[
        "/Create",
        "/TN",
        &logon_name,
        "/TR",
        &tr,
        "/SC",
        "ONLOGON",
        "/F",
        "/RL",
        "LIMITED",
    ]);

    Ok(serde_json::json!({
        "ok": true,
        "platform": "windows",
        "task_name": WIN_TASK_NAME,
        "binary": bin_str,
    }))
}

#[cfg(target_os = "windows")]
fn disable_autostart() -> Result<serde_json::Value, String> {
    let _ = run_schtasks(&["/Delete", "/TN", WIN_TASK_NAME, "/F"]);
    let _ = run_schtasks(&["/Delete", "/TN", &format!("{}-Logon", WIN_TASK_NAME), "/F"]);
    Ok(serde_json::json!({"ok": true, "platform": "windows", "removed": [WIN_TASK_NAME]}))
}

#[cfg(target_os = "windows")]
fn autostart_status() -> Result<serde_json::Value, String> {
    let out = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", WIN_TASK_NAME, "/FO", "CSV", "/NH"])
        .output()
        .map_err(|e| format!("schtasks: {}", e))?;
    let enabled = out.status.success() && !out.stdout.is_empty();
    Ok(serde_json::json!({
        "ok": true,
        "platform": "windows",
        "enabled": enabled,
        "task_name": WIN_TASK_NAME,
    }))
}

#[cfg(target_os = "windows")]
fn run_schtasks(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("schtasks")
        .args(args)
        .output()
        .map_err(|e| format!("schtasks spawn: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "schtasks {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

// ────────────────────────── macOS ──────────────────────────

#[cfg(target_os = "macos")]
const MAC_LABEL: &str = "com.codeterm.app";

#[cfg(target_os = "macos")]
fn mac_plist_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", MAC_LABEL)))
}

#[cfg(target_os = "macos")]
fn enable_autostart(binary_override: Option<&str>) -> Result<serde_json::Value, String> {
    let bin = resolve_app_binary(binary_override)?;
    let plist_path = mac_plist_path()?;
    let body = build_macos_plist(MAC_LABEL, &bin.to_string_lossy());

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&plist_path, &body)
        .map_err(|e| format!("write {}: {}", plist_path.display(), e))?;

    // unload may fail if not loaded — fine.
    let _ = std::process::Command::new("launchctl")
        .args(["unload", &plist_path.to_string_lossy()])
        .output();
    let out = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()
        .map_err(|e| format!("launchctl: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "launchctl load failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(serde_json::json!({
        "ok": true,
        "platform": "macos",
        "label": MAC_LABEL,
        "plist": plist_path.to_string_lossy(),
        "binary": bin.to_string_lossy(),
    }))
}

#[cfg(target_os = "macos")]
fn disable_autostart() -> Result<serde_json::Value, String> {
    let plist_path = mac_plist_path()?;
    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .output();
        std::fs::remove_file(&plist_path)
            .map_err(|e| format!("unlink {}: {}", plist_path.display(), e))?;
    }
    Ok(serde_json::json!({"ok": true, "platform": "macos", "label": MAC_LABEL}))
}

#[cfg(target_os = "macos")]
fn autostart_status() -> Result<serde_json::Value, String> {
    let plist_path = mac_plist_path()?;
    let exists = plist_path.exists();
    let loaded = std::process::Command::new("launchctl")
        .args(["list", MAC_LABEL])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    Ok(serde_json::json!({
        "ok": true,
        "platform": "macos",
        "enabled": exists && loaded,
        "label": MAC_LABEL,
        "plist": plist_path.to_string_lossy(),
        "loaded": loaded,
    }))
}

#[cfg(any(target_os = "macos", test))]
fn build_macos_plist(label: &str, binary_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary_path}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

// ────────────────────────── Linux ──────────────────────────

#[cfg(target_os = "linux")]
const LINUX_UNIT_NAME: &str = "codeterm.service";

#[cfg(target_os = "linux")]
fn linux_unit_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join(LINUX_UNIT_NAME))
}

#[cfg(target_os = "linux")]
fn enable_autostart(binary_override: Option<&str>) -> Result<serde_json::Value, String> {
    let bin = resolve_app_binary(binary_override)?;
    let unit_path = linux_unit_path()?;
    let body = build_linux_unit(&bin.to_string_lossy());

    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&unit_path, &body)
        .map_err(|e| format!("write {}: {}", unit_path.display(), e))?;

    run_systemctl(&["daemon-reload"])?;
    run_systemctl(&["enable", LINUX_UNIT_NAME])?;
    run_systemctl(&["start", LINUX_UNIT_NAME])?;

    Ok(serde_json::json!({
        "ok": true,
        "platform": "linux",
        "unit": LINUX_UNIT_NAME,
        "path": unit_path.to_string_lossy(),
        "binary": bin.to_string_lossy(),
    }))
}

#[cfg(target_os = "linux")]
fn disable_autostart() -> Result<serde_json::Value, String> {
    let unit_path = linux_unit_path()?;
    let _ = run_systemctl(&["stop", LINUX_UNIT_NAME]);
    let _ = run_systemctl(&["disable", LINUX_UNIT_NAME]);
    if unit_path.exists() {
        std::fs::remove_file(&unit_path)
            .map_err(|e| format!("unlink {}: {}", unit_path.display(), e))?;
    }
    let _ = run_systemctl(&["daemon-reload"]);
    Ok(serde_json::json!({"ok": true, "platform": "linux", "unit": LINUX_UNIT_NAME}))
}

#[cfg(target_os = "linux")]
fn autostart_status() -> Result<serde_json::Value, String> {
    let unit_path = linux_unit_path()?;
    let exists = unit_path.exists();
    let active = std::process::Command::new("systemctl")
        .args(["--user", "is-active", LINUX_UNIT_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let enabled = std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", LINUX_UNIT_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    Ok(serde_json::json!({
        "ok": true,
        "platform": "linux",
        "enabled": exists && enabled,
        "active": active,
        "unit": LINUX_UNIT_NAME,
        "path": unit_path.to_string_lossy(),
    }))
}

#[cfg(target_os = "linux")]
fn run_systemctl(args: &[&str]) -> Result<(), String> {
    let mut full: Vec<&str> = vec!["--user"];
    full.extend(args.iter().copied());
    let out = std::process::Command::new("systemctl")
        .args(&full)
        .output()
        .map_err(|e| format!("systemctl spawn: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "systemctl {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn build_linux_unit(binary_path: &str) -> String {
    format!(
        "[Unit]\n\
         Description=CodeTerm — terminal multiplexer for AI agents\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary_path}\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_plist_contains_label_and_binary_and_keepalive_and_runatload() {
        let plist = build_macos_plist(
            "com.codeterm.app",
            "/Applications/CodeTerm.app/Contents/MacOS/CodeTerm",
        );
        assert!(plist.contains("<string>com.codeterm.app</string>"));
        assert!(plist.contains("/Applications/CodeTerm.app/Contents/MacOS/CodeTerm"));
        assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    }

    #[test]
    fn linux_unit_has_restart_on_failure_and_user_target() {
        let unit = build_linux_unit("/usr/local/bin/codeterm");
        assert!(unit.contains("ExecStart=/usr/local/bin/codeterm"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("[Install]"));
    }

    #[test]
    fn linux_unit_handles_path_with_spaces() {
        let unit = build_linux_unit("/opt/Code Term/codeterm");
        assert!(unit.contains("ExecStart=/opt/Code Term/codeterm"));
    }

    #[test]
    fn macos_plist_escapes_path_with_spaces_inside_xml_string() {
        // We don't escape — XML <string> happily contains spaces. This pins the assumption so a future "helpful" change doesn't add shell-style
        // quoting that would break the plist parser.
        let plist = build_macos_plist(
            "com.codeterm.app",
            "/Applications/Code Term.app/Contents/MacOS/CodeTerm",
        );
        assert!(
            plist.contains("<string>/Applications/Code Term.app/Contents/MacOS/CodeTerm</string>")
        );
    }
}
