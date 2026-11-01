//! `domios system` — OS-level autostart triggers (schtasks / launchd /
//! systemd). Explicit user invocation only; never auto-enabled.

use crate::SystemAction;

mod installed;
mod registration;

use installed::{canonical_file, AutostartState, InstalledSupervisor};

pub fn handle_system(json_mode: bool, action: SystemAction) {
    if matches!(action, SystemAction::ApiToken) {
        handle_api_token(false);
        return;
    }
    let result: Result<serde_json::Value, String> = match action {
        SystemAction::EnableAutostart { binary, supervisor } => {
            enable_autostart(binary.as_deref(), supervisor.as_deref())
        }
        SystemAction::DisableAutostart => disable_autostart(),
        SystemAction::AutostartStatus => autostart_status(),
        SystemAction::Supervise { binary, once } => supervise(binary.as_deref(), once),
        SystemAction::ApiToken => unreachable!("handled before result rendering"),
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
    InstalledSupervisor::discover()
        .resolve(override_path.map(std::path::Path::new), canonical_file)
        .map_err(|reason| format!(
            "stable installed Domios CLI supervisor unavailable ({reason:?}); open the installed Domios app to restore its CLI, then run `{}`; --supervisor must select an installed CLI outside temporary directories",
            installed::REPAIR_COMMAND,
        ))
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
    _file: std::fs::File,
}

impl SupervisorLock {
    fn acquire() -> Result<Self, String> {
        let path = recovery_dir().join("supervisor.lock");
        Self::acquire_at(&path)
    }

    fn acquire_at(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| format!("open {}: {error}", path.display()))?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => format!(
                "another Domios supervisor is already active: {}",
                path.display()
            ),
            std::fs::TryLockError::Error(error) => {
                format!("lock {}: {error}", path.display())
            }
        })?;
        // Keep the inode and handle: unlinking permits a second lock on a replacement file.
        use std::io::Write;
        file.set_len(0).map_err(|error| error.to_string())?;
        writeln!(file, "{}", std::process::id()).map_err(|error| error.to_string())?;
        Ok(Self { _file: file })
    }
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

fn installed_app_candidates(
    platform: &str,
    local_app_data: Option<std::path::PathBuf>,
    program_files: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    match platform {
        "windows" => [local_app_data, program_files]
            .into_iter()
            .flatten()
            .map(|root| root.join("Domios/domios-app.exe"))
            .collect(),
        "macos" => std::iter::once(std::path::PathBuf::from(
            "/Applications/Domios.app/Contents/MacOS/domios-app",
        ))
        .chain(home.map(|root| root.join("Applications/Domios.app/Contents/MacOS/domios-app")))
        .collect(),
        "linux" => ["/usr/local/bin/domios-app", "/usr/bin/domios-app"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .chain(home.map(|root| root.join(".local/bin/domios-app")))
            .collect(),
        _ => Vec::new(),
    }
}

fn default_app_binary() -> Result<std::path::PathBuf, String> {
    installed_app_candidates(
        std::env::consts::OS,
        std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from),
        std::env::var_os("ProgramFiles").map(std::path::PathBuf::from),
        dirs::home_dir(),
    )
    .into_iter()
    .find(|path| path.is_file())
    .ok_or_else(|| {
        "Domios application executable not found; pass --binary to select its installed path".into()
    })
}

fn autostart_home() -> Result<std::path::PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "home dir not found".into())
}

fn enable_autostart(
    binary_override: Option<&str>,
    supervisor_override: Option<&str>,
) -> Result<serde_json::Value, String> {
    let bin = resolve_app_binary(binary_override)?;
    let supervisor = resolve_supervisor_binary(supervisor_override)?;
    let home = autostart_home()?;
    codeterm_util::autostart::enable(&home, &crate::data_root(), &supervisor, &bin)?;
    let mut result = serde_json::json!({"ok":true,"platform":platform_name(),"binary":bin.to_string_lossy(),"supervisor":supervisor.to_string_lossy()});
    #[cfg(windows)]
    {
        result["task_name"] = WIN_TASK_NAME.into();
    }
    #[cfg(target_os = "macos")]
    {
        result["label"] = MAC_LABEL.into();
        result["plist"] = mac_plist_path()?.to_string_lossy().into_owned().into();
    }
    #[cfg(target_os = "linux")]
    {
        result["unit"] = LINUX_UNIT_NAME.into();
        result["path"] = linux_unit_path()?.to_string_lossy().into_owned().into();
    }
    Ok(result)
}

fn disable_autostart() -> Result<serde_json::Value, String> {
    codeterm_util::autostart::disable(&autostart_home()?, &crate::data_root())?;
    let mut result = serde_json::json!({"ok":true,"platform":platform_name()});
    #[cfg(windows)]
    {
        result["removed"] = serde_json::json!([WIN_TASK_NAME]);
    }
    #[cfg(target_os = "macos")]
    {
        result["label"] = MAC_LABEL.into();
    }
    #[cfg(target_os = "linux")]
    {
        result["unit"] = LINUX_UNIT_NAME.into();
    }
    Ok(result)
}

#[cfg(windows)]
const WIN_TASK_NAME: &str = codeterm_util::autostart::WINDOWS_TASK;
#[cfg(target_os = "macos")]
const MAC_LABEL: &str = codeterm_util::autostart::MACOS_LABEL;
#[cfg(target_os = "linux")]
const LINUX_UNIT_NAME: &str = codeterm_util::autostart::LINUX_UNIT;

#[cfg(target_os = "macos")]
fn mac_plist_path() -> Result<std::path::PathBuf, String> {
    Ok(codeterm_util::autostart::macos_path(&autostart_home()?))
}
#[cfg(target_os = "linux")]
fn linux_unit_path() -> Result<std::path::PathBuf, String> {
    Ok(codeterm_util::autostart::linux_path(&autostart_home()?))
}

#[cfg(target_os = "windows")]
fn autostart_status() -> Result<serde_json::Value, String> {
    let out = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", WIN_TASK_NAME, "/XML"])
        .output()
        .map_err(|e| format!("schtasks: {}", e))?;
    let enabled = out.status.success() && !out.stdout.is_empty();
    let raw = registration::windows_task_text(&out.stdout);
    let target = registration::windows_target(&raw);
    let state = AutostartState::assess(
        enabled,
        enabled,
        target.as_deref(),
        &InstalledSupervisor::discover(),
        canonical_file,
    );
    Ok(state.apply(serde_json::json!({
        "ok": true,
        "platform": "windows",
        "enabled": enabled,
        "task_name": WIN_TASK_NAME,
        "supervisor": target,
    })))
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
    let target = std::fs::read_to_string(&plist_path)
        .ok()
        .and_then(|raw| registration::macos_target(&raw));
    let state = AutostartState::assess(
        exists || loaded,
        exists && loaded,
        target.as_deref(),
        &InstalledSupervisor::discover(),
        canonical_file,
    );
    Ok(state.apply(serde_json::json!({
        "ok": true,
        "platform": "macos",
        "enabled": exists && loaded,
        "label": MAC_LABEL,
        "plist": plist_path.to_string_lossy(),
        "loaded": loaded,
        "supervisor": target,
    })))
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
    let target = std::fs::read_to_string(&unit_path)
        .ok()
        .and_then(|raw| registration::linux_target(&raw));
    let state = AutostartState::assess(
        exists || enabled || active,
        exists && enabled,
        target.as_deref(),
        &InstalledSupervisor::discover(),
        canonical_file,
    );
    Ok(state.apply(serde_json::json!({
        "ok": true,
        "platform": "linux",
        "enabled": exists && enabled,
        "active": active,
        "unit": LINUX_UNIT_NAME,
        "path": unit_path.to_string_lossy(),
        "supervisor": target,
    })))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
fn instance_api_token(data_root: &std::path::Path, remote: bool) -> Result<String, String> {
    super::host_credentials::read(data_root, remote, None)?
        .ok_or_else(|| "No local instance API credential; start Domios first".into())
}

#[cfg(test)]
#[path = "system/token_tests.rs"]
mod token_tests;

pub fn handle_api_token(remote: bool) {
    let caller = crate::nonempty_env("CODETERM_TAB_ID");
    let token = super::host_credentials::read(&crate::data_root(), remote, caller.as_deref())
        .and_then(|token| {
            token.ok_or_else(|| "No local instance API credential; start Domios first".into())
        });
    match token {
        Ok(token) if crate::cmd::error_contract::json_mode() => {
            crate::cmd::print::print_json(&serde_json::json!(token))
        }
        Ok(token) => print!("{token}"),
        Err(error) => crate::cmd::print::fail(if remote { 7 } else { 1 }, error),
    }
}
