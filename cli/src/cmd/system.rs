//! `codeterm system` — OS-level autostart triggers (schtasks / launchd /
//! systemd). Explicit user invocation only; never auto-enabled.

use crate::SystemAction;

pub fn handle_system(json_mode: bool, action: SystemAction) {
    let result: Result<serde_json::Value, String> = match action {
        SystemAction::EnableAutostart { binary, supervisor } => {
            enable_autostart(binary.as_deref(), supervisor.as_deref())
        }
        SystemAction::DisableAutostart => disable_autostart(),
        SystemAction::AutostartStatus => autostart_status(),
        SystemAction::Supervise { binary, once } => supervise(binary.as_deref(), once),
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

fn resolve_supervisor_binary(override_path: Option<&str>) -> Result<std::path::PathBuf, String> {
    if let Some(p) = override_path {
        let path = std::path::PathBuf::from(p);
        if !path.exists() {
            return Err(format!("override supervisor not found: {}", path.display()));
        }
        return Ok(path);
    }
    std::env::current_exe().map_err(|e| e.to_string())
}

const LOOP_WINDOW_SECS: i64 = 300;
const LOOP_THRESHOLD: u32 = 3;
const MAX_BACKOFF_SECS: u64 = 30;
const LIVE_HEARTBEAT_SECS: i64 = 30;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoveryState {
    current: ProcessEpoch,
    last_clean_shutdown: Option<CleanShutdownRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ProcessEpoch {
    epoch: String,
    pid: u32,
    started_at: i64,
    last_healthy_at: i64,
    build_hash: String,
    #[serde(default = "platform_name")]
    platform: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CrashRecord {
    detected_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SupervisorAction {
    #[serde(default = "default_true")]
    external_restart_required: bool,
    backoff_ms: u64,
    loop_protected: bool,
    #[serde(default)]
    crash_count_in_window: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CleanShutdownRecord {
    epoch: Option<String>,
    reason: String,
    #[serde(default)]
    pid: u32,
    at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingCrashRecord {
    classification: String,
    previous: ProcessEpoch,
    detected_at: i64,
    supervisor_action: SupervisorAction,
}

fn supervise(binary_override: Option<&str>, once: bool) -> Result<serde_json::Value, String> {
    let bin = resolve_app_binary(binary_override)?;
    let _lock = SupervisorLock::acquire()?;
    let mut launches = 0u32;

    if supervisor_is_disabled() {
        return supervisor_result("disabled", launches, None);
    }

    if let Some(state) = read_recovery_state().filter(current_process_is_live) {
        let observed = state.current.clone();
        let observed_pid = observed.pid;
        wait_for_process_exit(observed.pid);
        if finish_observation(observed)?.is_none() {
            return supervisor_result("clean_shutdown", launches, Some(observed_pid));
        }
    } else if read_pending_crash().is_none() {
        if let Some(state) = read_recovery_state() {
            if !clean_matches(state.last_clean_shutdown.as_ref(), &state.current) {
                persist_pending_crash(&state.current)?;
            }
        }
    }

    loop {
        if supervisor_is_disabled() {
            return supervisor_result("disabled", launches, None);
        }
        let pending = read_pending_crash();
        if pending
            .as_ref()
            .is_some_and(|p| p.supervisor_action.loop_protected)
        {
            return supervisor_result("loop_protected", launches, None);
        }
        let delay = pending
            .as_ref()
            .map(|p| p.supervisor_action.backoff_ms)
            .unwrap_or(0);
        if delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }

        launches += 1;
        let launched_at = now_ts();
        let mut child = launch_codeterm(&bin)?;
        let pid = child.id();
        let _status = child
            .wait()
            .map_err(|e| format!("wait for {}: {}", bin.display(), e))?;
        let after = read_recovery_state();
        let observed = observed_epoch(after.as_ref(), pid, launched_at);
        let crash = finish_observation(observed)?;
        if crash.is_none() {
            return supervisor_result("clean_shutdown", launches, Some(pid));
        }
        let crash = crash.expect("checked above");
        if supervisor_is_disabled() {
            return supervisor_result("disabled", launches, Some(pid));
        }
        if crash.supervisor_action.loop_protected {
            return supervisor_result("loop_protected", launches, Some(pid));
        }
        if once {
            return supervisor_result("once", launches, Some(pid));
        }
    }
}

fn launch_codeterm(bin: &std::path::Path) -> Result<std::process::Child, String> {
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("--supervised");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map_err(|e| format!("launch {}: {}", bin.display(), e))
}

fn current_process_is_live(state: &RecoveryState) -> bool {
    now_ts().saturating_sub(state.current.last_healthy_at) <= LIVE_HEARTBEAT_SECS
        && process_exists(state.current.pid)
}

fn wait_for_process_exit(pid: u32) {
    while process_exists(pid) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn finish_observation(mut observed: ProcessEpoch) -> Result<Option<PendingCrashRecord>, String> {
    loop {
        let clean = read_clean_shutdown();
        if !clean_matches(clean.as_ref(), &observed) {
            return persist_pending_crash(&observed).map(Some);
        }
        remove_pending_crash();
        let Some(clean) = clean.filter(should_follow_replacement) else {
            return Ok(None);
        };
        let Some(replacement) = wait_for_replacement(&observed, clean.at) else {
            return Ok(None);
        };
        observed = replacement;
        wait_for_process_exit(observed.pid);
    }
}

fn should_follow_replacement(clean: &CleanShutdownRecord) -> bool {
    matches!(clean.reason.as_str(), "restart" | "update")
}

fn wait_for_replacement(previous: &ProcessEpoch, clean_at: i64) -> Option<ProcessEpoch> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if let Some(current) = read_recovery_state().map(|state| state.current) {
            if current.epoch != previous.epoch
                && current.started_at >= clean_at
                && current_process_is_live(&RecoveryState {
                    current: current.clone(),
                    last_clean_shutdown: None,
                })
            {
                return Some(current);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    None
}

fn observed_epoch(state: Option<&RecoveryState>, pid: u32, launched_at: i64) -> ProcessEpoch {
    state
        .filter(|state| state.current.pid == pid && state.current.started_at >= launched_at)
        .map(|state| state.current.clone())
        .unwrap_or_else(|| ProcessEpoch {
            epoch: format!("supervisor-{}", uuid::Uuid::new_v4()),
            pid,
            started_at: launched_at,
            last_healthy_at: launched_at,
            build_hash: "startup-not-reached".into(),
            platform: platform_name(),
        })
}

fn clean_matches(clean: Option<&CleanShutdownRecord>, process: &ProcessEpoch) -> bool {
    clean.is_some_and(|clean| {
        clean.epoch.as_deref() == Some(process.epoch.as_str())
            && clean.pid == process.pid
            && clean.at >= process.started_at
    })
}

fn persist_pending_crash(previous: &ProcessEpoch) -> Result<PendingCrashRecord, String> {
    let now = now_ts();
    let crash_count = crash_count_in_window(now) + 1;
    let record = PendingCrashRecord {
        classification: classify(previous, now).into(),
        previous: previous.clone(),
        detected_at: now,
        supervisor_action: SupervisorAction {
            external_restart_required: true,
            backoff_ms: bounded_backoff_ms(crash_count),
            loop_protected: crash_count >= LOOP_THRESHOLD,
            crash_count_in_window: crash_count,
        },
    };
    write_json(&pending_crash_path(), &record)?;
    Ok(record)
}

fn classify(previous: &ProcessEpoch, now: i64) -> &'static str {
    if now.saturating_sub(previous.last_healthy_at) > LOOP_WINDOW_SECS {
        "power_loss_or_kill"
    } else if now.saturating_sub(previous.started_at) < 30 {
        "fast_fail"
    } else {
        "abnormal_exit"
    }
}

fn bounded_backoff_ms(crash_count: u32) -> u64 {
    let shift = crash_count.saturating_sub(1).min(5);
    (1_000u64 << shift).min(MAX_BACKOFF_SECS * 1_000)
}

fn crash_count_in_window(now: i64) -> u32 {
    let history_count = read_json::<Vec<CrashRecord>>(&recovery_dir().join("crash_history.json"))
        .unwrap_or_default()
        .into_iter()
        .filter(|record| now.saturating_sub(record.detected_at) <= LOOP_WINDOW_SECS)
        .count() as u32;
    let pending_count = read_pending_crash()
        .filter(|record| now.saturating_sub(record.detected_at) <= LOOP_WINDOW_SECS)
        .map(|_| 1)
        .unwrap_or(0);
    history_count + pending_count
}

fn read_recovery_state() -> Option<RecoveryState> {
    let path = recovery_dir().join("current.json");
    let current: ProcessEpoch = read_json(&path)?;
    let last_clean_shutdown = read_clean_shutdown();
    Some(RecoveryState {
        current,
        last_clean_shutdown,
    })
}

fn recovery_dir() -> std::path::PathBuf {
    crate::data_root().join("state").join("recovery")
}

fn pending_crash_path() -> std::path::PathBuf {
    recovery_dir().join("pending_crash.json")
}

fn supervisor_disabled_path() -> std::path::PathBuf {
    recovery_dir().join("supervisor.disabled")
}

fn supervisor_is_disabled() -> bool {
    supervisor_disabled_path().exists()
}

fn mark_supervisor_enabled() -> Result<(), String> {
    match std::fs::remove_file(supervisor_disabled_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn mark_supervisor_disabled() -> Result<(), String> {
    write_json(
        &supervisor_disabled_path(),
        &serde_json::json!({ "disabled_at": now_ts() }),
    )
}

fn read_clean_shutdown() -> Option<CleanShutdownRecord> {
    read_json(&recovery_dir().join("last_clean.json"))
}

fn read_pending_crash() -> Option<PendingCrashRecord> {
    read_json(&pending_crash_path())
}

fn remove_pending_crash() {
    let _ = std::fs::remove_file(pending_crash_path());
}

fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &std::path::Path) -> Option<T> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    let raw = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    file.write_all(&raw)
        .and_then(|_| file.sync_data())
        .map_err(|e| format!("write {}: {}", path.display(), e))
}

fn supervisor_result(
    stopped: &str,
    launches: u32,
    last_pid: Option<u32>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "ok": true,
        "stopped": stopped,
        "launches": launches,
        "last_pid": last_pid,
        "pending_crash": read_pending_crash(),
        "current": read_recovery_state().map(|state| state.current),
    }))
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn platform_name() -> String {
    std::env::consts::OS.to_string()
}

fn default_true() -> bool {
    true
}

struct SupervisorLock {
    path: std::path::PathBuf,
}

impl SupervisorLock {
    fn acquire() -> Result<Self, String> {
        let path = recovery_dir().join("supervisor.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                writeln!(file, "{}", std::process::id()).map_err(|e| e.to_string())?;
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if stale_lock(&path) {
                    let _ = std::fs::remove_file(&path);
                    return Self::acquire();
                }
                Err(format!(
                    "another CodeTerm supervisor is already active: {}",
                    path.display()
                ))
            }
            Err(e) => Err(format!("create {}: {}", path.display(), e)),
        }
    }
}

impl Drop for SupervisorLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn stale_lock(path: &std::path::Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return false;
    };
    !process_exists(pid)
}

#[cfg(target_os = "windows")]
fn process_exists(pid: u32) -> bool {
    let expected = pid.to_string();
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|token| token == expected)
        })
        .unwrap_or(true)
}

#[cfg(not(target_os = "windows"))]
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(true)
}

#[cfg(target_os = "windows")]
fn default_app_binary() -> Result<std::path::PathBuf, String> {
    let candidates = [
        std::env::var_os("LOCALAPPDATA").map(|p| {
            std::path::PathBuf::from(p)
                .join("codeterm")
                .join("codeterm.exe")
        }),
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
        std::path::PathBuf::from("/Applications/CodeTerm.app/Contents/MacOS/codeterm"),
        std::path::PathBuf::from("/Applications/CodeTerm.app/Contents/MacOS/CodeTerm"),
        dirs::home_dir()
            .map(|h| h.join("Applications/CodeTerm.app/Contents/MacOS/codeterm"))
            .unwrap_or_default(),
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
fn enable_autostart(
    binary_override: Option<&str>,
    supervisor_override: Option<&str>,
) -> Result<serde_json::Value, String> {
    mark_supervisor_enabled()?;
    let bin = resolve_app_binary(binary_override)?;
    let supervisor = resolve_supervisor_binary(supervisor_override)?;
    let bin_str = bin.to_string_lossy().to_string();
    let supervisor_str = supervisor.to_string_lossy().to_string();
    let tr = build_windows_task_command(&supervisor_str, &bin_str);

    let _ = run_schtasks(&["/Delete", "/TN", WIN_TASK_NAME, "/F"]);
    run_schtasks(&[
        "/Create",
        "/TN",
        WIN_TASK_NAME,
        "/TR",
        &tr,
        "/SC",
        "ONLOGON",
        "/F",
        "/RL",
        "LIMITED",
    ])?;
    run_schtasks(&["/Run", "/TN", WIN_TASK_NAME])?;

    Ok(serde_json::json!({
        "ok": true,
        "platform": "windows",
        "task_name": WIN_TASK_NAME,
        "binary": bin_str,
        "supervisor": supervisor_str,
    }))
}

#[cfg(target_os = "windows")]
fn disable_autostart() -> Result<serde_json::Value, String> {
    mark_supervisor_disabled()?;
    let _ = run_schtasks(&["/End", "/TN", WIN_TASK_NAME]);
    let _ = run_schtasks(&["/Delete", "/TN", WIN_TASK_NAME, "/F"]);
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

#[cfg(any(target_os = "windows", test))]
fn build_windows_task_command(supervisor_path: &str, app_binary: &str) -> String {
    format!(
        "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"Start-Process -WindowStyle Hidden -FilePath '{}' -ArgumentList @('system','supervise','--binary','{}')\"",
        supervisor_path.replace('\'', "''"),
        app_binary.replace('\'', "''")
    )
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
fn enable_autostart(
    binary_override: Option<&str>,
    supervisor_override: Option<&str>,
) -> Result<serde_json::Value, String> {
    mark_supervisor_enabled()?;
    let bin = resolve_app_binary(binary_override)?;
    let supervisor = resolve_supervisor_binary(supervisor_override)?;
    let plist_path = mac_plist_path()?;
    let body = build_macos_plist(
        MAC_LABEL,
        &supervisor.to_string_lossy(),
        &bin.to_string_lossy(),
    );

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
        "supervisor": supervisor.to_string_lossy(),
    }))
}

#[cfg(target_os = "macos")]
fn disable_autostart() -> Result<serde_json::Value, String> {
    mark_supervisor_disabled()?;
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
fn build_macos_plist(label: &str, supervisor_path: &str, binary_path: &str) -> String {
    let label = xml_escape(label);
    let supervisor_path = xml_escape(supervisor_path);
    let binary_path = xml_escape(binary_path);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{supervisor_path}</string>
        <string>system</string>
        <string>supervise</string>
        <string>--binary</string>
        <string>{binary_path}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
fn enable_autostart(
    binary_override: Option<&str>,
    supervisor_override: Option<&str>,
) -> Result<serde_json::Value, String> {
    mark_supervisor_enabled()?;
    let bin = resolve_app_binary(binary_override)?;
    let supervisor = resolve_supervisor_binary(supervisor_override)?;
    let unit_path = linux_unit_path()?;
    let body = build_linux_unit(&supervisor.to_string_lossy(), &bin.to_string_lossy());

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
        "supervisor": supervisor.to_string_lossy(),
    }))
}

#[cfg(target_os = "linux")]
fn disable_autostart() -> Result<serde_json::Value, String> {
    mark_supervisor_disabled()?;
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
fn build_linux_unit(supervisor_path: &str, binary_path: &str) -> String {
    let supervisor_path = systemd_quote(supervisor_path);
    let binary_path = systemd_quote(binary_path);
    format!(
        "[Unit]\n\
         Description=CodeTerm — terminal multiplexer for AI agents\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={supervisor_path} system supervise --binary {binary_path}\n\
         Restart=no\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

#[cfg(any(target_os = "linux", test))]
fn systemd_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_plist_contains_label_and_binary_and_keepalive_and_runatload() {
        let plist = build_macos_plist(
            "com.codeterm.app",
            "/Users/me/.codeterm/bin/codeterm",
            "/Applications/CodeTerm.app/Contents/MacOS/CodeTerm",
        );
        assert!(plist.contains("<string>com.codeterm.app</string>"));
        assert!(plist.contains("<string>/Users/me/.codeterm/bin/codeterm</string>"));
        assert!(plist.contains("<string>system</string>"));
        assert!(plist.contains("<string>supervise</string>"));
        assert!(plist.contains("/Applications/CodeTerm.app/Contents/MacOS/CodeTerm"));
        assert!(plist.contains("<key>KeepAlive</key>\n    <false/>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    }

    #[test]
    fn linux_unit_runs_supervisor_and_user_target() {
        let unit = build_linux_unit("/home/me/.codeterm/bin/codeterm", "/usr/local/bin/codeterm");
        assert!(unit.contains(
            "ExecStart=\"/home/me/.codeterm/bin/codeterm\" system supervise --binary \"/usr/local/bin/codeterm\""
        ));
        assert!(unit.contains("Restart=no"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("[Install]"));
    }

    #[test]
    fn linux_unit_handles_path_with_spaces() {
        let unit = build_linux_unit("/home/me/Code Term/codeterm", "/opt/Code Term/codeterm");
        assert!(unit.contains(
            "ExecStart=\"/home/me/Code Term/codeterm\" system supervise --binary \"/opt/Code Term/codeterm\""
        ));
    }

    #[test]
    fn macos_plist_escapes_xml_metacharacters() {
        let plist = build_macos_plist(
            "com.codeterm.app",
            "/Users/me/Code & Term/codeterm",
            "/Applications/Code <Term>.app/Contents/MacOS/CodeTerm",
        );
        assert!(plist.contains("/Users/me/Code &amp; Term/codeterm"));
        assert!(plist.contains("/Applications/Code &lt;Term&gt;.app"));
    }

    #[test]
    fn windows_task_runs_supervisor_not_app_directly() {
        let command = build_windows_task_command(
            r"C:\Users\me\.codeterm\bin\codeterm.exe",
            r"C:\Users\me\AppData\Local\codeterm\codeterm.exe",
        );
        assert!(command.contains("system"));
        assert!(command.contains("supervise"));
        assert!(command.contains("--binary"));
        assert!(command.contains(r"C:\Users\me\.codeterm\bin\codeterm.exe"));
        assert!(command.contains(r"C:\Users\me\AppData\Local\codeterm\codeterm.exe"));
    }

    #[test]
    fn supervisor_stops_only_for_clean_matching_identity() {
        let state = recovery_state("epoch-1", Some(("epoch-1", "user_exit")));
        assert!(clean_matches(
            state.last_clean_shutdown.as_ref(),
            &state.current
        ));
    }

    #[test]
    fn supervisor_does_not_stop_for_stale_clean_epoch() {
        let state = recovery_state("epoch-2", Some(("epoch-1", "update")));
        assert!(!clean_matches(
            state.last_clean_shutdown.as_ref(),
            &state.current
        ));
    }

    #[test]
    fn supervisor_classifies_pre_marker_exit_as_fast_fail() {
        let state = recovery_state("supervisor-launch", None);
        assert_eq!(classify(&state.current, 20), "fast_fail");
    }

    #[test]
    fn supervisor_follows_only_restart_and_update_replacements() {
        let restart = recovery_state("epoch-1", Some(("epoch-1", "restart")));
        let user_exit = recovery_state("epoch-1", Some(("epoch-1", "user_exit")));
        assert!(should_follow_replacement(
            restart.last_clean_shutdown.as_ref().unwrap()
        ));
        assert!(!should_follow_replacement(
            user_exit.last_clean_shutdown.as_ref().unwrap()
        ));
    }

    #[test]
    fn supervisor_backoff_is_bounded() {
        assert_eq!(bounded_backoff_ms(1), 1_000);
        assert_eq!(bounded_backoff_ms(3), 4_000);
        assert_eq!(bounded_backoff_ms(99), 30_000);
    }

    fn recovery_state(epoch: &str, clean: Option<(&str, &str)>) -> RecoveryState {
        RecoveryState {
            current: ProcessEpoch {
                epoch: epoch.into(),
                pid: 42,
                started_at: 10,
                last_healthy_at: 20,
                build_hash: "abc123".into(),
                platform: "windows".into(),
            },
            last_clean_shutdown: clean.map(|(epoch, reason)| CleanShutdownRecord {
                epoch: Some(epoch.into()),
                reason: reason.into(),
                pid: 42,
                at: 40,
            }),
        }
    }
}
