use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

const EVENT_CHANGED: &str = "keepawake-changed";
const SUPERVISOR_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

const PERSIST_FILE: &str = "keepawake.json";
const CHILDREN_FILE: &str = "keepawake-children.json";

pub type KeepAwakeState = Arc<Mutex<Option<ActiveSession>>>;

pub fn new_state() -> KeepAwakeState {
    Arc::new(Mutex::new(None))
}

pub struct ActiveSession {
    stop_tx: std::sync::mpsc::Sender<()>,
    display: bool,
    expires_at: Option<i64>,
    session_id: u64,
    gnome_policy: bool,
    system_block: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct KeepAwakeChild {
    pid: u32,
    spawned_at_ms: i64,
    display: bool,
    wrapper: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessSnapshot {
    ppid: u32,
    command: String,
}

static CHILD_REGISTRY: OnceLock<Mutex<Vec<KeepAwakeChild>>> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone)]
pub struct KeepAwakeStatus {
    pub active: bool,
    pub display: bool,
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<KeepAwakeDiagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admin_commands: Vec<String>,
    #[serde(default)]
    pub gnome_policy: bool,
    #[serde(default)]
    pub system_block: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct KeepAwakeDiagnostic {
    pub runtime_inhibit_active: bool,
    pub display_inhibit_active: bool,
    pub gnome_policy_safe: Option<bool>,
    pub systemd_hard_block_active: Option<bool>,
    pub systemd_daemon_reload_needed: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsafe_gnome_settings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_systemd_masks: Vec<String>,
}

fn persist_path() -> PathBuf {
    crate::paths::data_dir().join(PERSIST_FILE)
}

fn children_path() -> PathBuf {
    crate::paths::data_dir().join(CHILDREN_FILE)
}

fn child_registry() -> &'static Mutex<Vec<KeepAwakeChild>> {
    CHILD_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn load_children_from(path: &std::path::Path) -> Vec<KeepAwakeChild> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<KeepAwakeChild>>(&bytes).unwrap_or_default()
}

fn save_children_to(path: &std::path::Path, children: &[KeepAwakeChild]) {
    if children.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let bytes = match serde_json::to_vec(children) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("[keepawake] failed to serialize child registry: {}", e);
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, bytes) {
        log::warn!("[keepawake] failed to write child registry tmp: {}", e);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        log::warn!("[keepawake] failed to rename child registry: {}", e);
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn register_child(pid: u32, display: bool, wrapper: bool) {
    let child = KeepAwakeChild {
        pid,
        spawned_at_ms: now_ms(),
        display,
        wrapper,
    };
    let path = children_path();
    let mut guard = child_registry().lock();
    guard.retain(|c| c.pid != pid);
    guard.push(child);
    save_children_to(&path, &guard);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unregister_child(pid: u32) {
    let path = children_path();
    let mut guard = child_registry().lock();
    guard.retain(|c| c.pid != pid);
    save_children_to(&path, &guard);
}

pub(crate) fn reap_orphaned_children_on_startup() {
    let path = children_path();
    let entries = load_children_from(&path);
    let outcome = reap_journaled_orphans(entries, inspect_process, terminate_pid);
    if !outcome.killed.is_empty() {
        log::warn!(
            "[keepawake] reaped orphaned keepawake child pid(s): {:?}",
            outcome.killed
        );
    }
    if !outcome.skipped.is_empty() {
        log::warn!(
            "[keepawake] skipped stale child registry pid(s): {:?}",
            outcome.skipped
        );
    }
    *child_registry().lock() = outcome.kept.clone();
    save_children_to(&path, &outcome.kept);
}

pub(crate) fn cleanup_active_children_for_exit() {
    let killed = cleanup_active_children_with_state(None, terminate_pid);
    if !killed.is_empty() {
        log::warn!("[keepawake] terminated active child pid(s): {:?}", killed);
    }
}

pub(crate) fn cleanup_active_children_for_exit_with_state(state: &KeepAwakeState) {
    let killed = cleanup_active_children_with_state(Some(state), terminate_pid);
    if !killed.is_empty() {
        log::warn!("[keepawake] terminated active child pid(s): {:?}", killed);
    }
}

fn cleanup_active_children_with_state<K>(state: Option<&KeepAwakeState>, mut kill: K) -> Vec<u32>
where
    K: FnMut(u32) -> Result<(), String>,
{
    if let Some(state) = state {
        let _ = state.lock().take();
    }
    let path = children_path();
    let mut guard = child_registry().lock();
    let entries = std::mem::take(&mut *guard);
    let killed = cleanup_child_entries(entries, &mut kill);
    save_children_to(&path, &[]);
    killed
}

fn cleanup_child_entries<K>(entries: Vec<KeepAwakeChild>, mut kill: K) -> Vec<u32>
where
    K: FnMut(u32) -> Result<(), String>,
{
    let mut killed = Vec::new();
    for child in entries {
        if kill(child.pid).is_ok() {
            killed.push(child.pid);
        }
    }
    killed
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ReapOutcome {
    killed: Vec<u32>,
    skipped: Vec<u32>,
    kept: Vec<KeepAwakeChild>,
}

fn reap_journaled_orphans<I, K>(
    entries: Vec<KeepAwakeChild>,
    inspect: I,
    mut kill: K,
) -> ReapOutcome
where
    I: Fn(u32) -> Option<ProcessSnapshot>,
    K: FnMut(u32) -> Result<(), String>,
{
    let mut outcome = ReapOutcome::default();
    for child in entries {
        let Some(snapshot) = inspect(child.pid) else {
            continue;
        };
        if should_reap_child(&snapshot) {
            if snapshot.ppid == 1 {
                match kill(child.pid) {
                    Ok(()) => outcome.killed.push(child.pid),
                    Err(e) => {
                        log::warn!("[keepawake] failed to terminate pid {}: {}", child.pid, e);
                        outcome.kept.push(child);
                    }
                }
            } else {
                outcome.kept.push(child);
            }
        } else {
            outcome.skipped.push(child.pid);
        }
    }
    outcome
}

fn should_reap_child(snapshot: &ProcessSnapshot) -> bool {
    snapshot.command.contains("codeterm-keepawake-wrapper")
        || snapshot.command.contains("caffeinate")
        || snapshot.command.contains("systemd-inhibit")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildHealth {
    alive: bool,
    warning: Option<String>,
}

fn active_child_health_with<I>(inspect: I) -> ChildHealth
where
    I: Fn(u32) -> Option<ProcessSnapshot>,
{
    let entries = child_registry().lock().clone();

    if entries.is_empty() {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        return ChildHealth {
            alive: false,
            warning: Some("keepawake child registry is empty".to_string()),
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        return ChildHealth {
            alive: true,
            warning: None,
        };
    }

    let missing: Vec<u32> = entries
        .iter()
        .filter(|child| {
            inspect(child.pid)
                .as_ref()
                .is_none_or(|snapshot| !should_reap_child(snapshot))
        })
        .map(|child| child.pid)
        .collect();

    if !missing.is_empty() {
        return ChildHealth {
            alive: false,
            warning: Some(format!("keepawake child process not alive: {:?}", missing)),
        };
    }

    ChildHealth {
        alive: true,
        warning: None,
    }
}

fn inspect_process(pid: u32) -> Option<ProcessSnapshot> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "ppid=", "-o", "command="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_ps_snapshot(&String::from_utf8_lossy(&output.stdout))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn parse_ps_snapshot(output: &str) -> Option<ProcessSnapshot> {
    let line = output.lines().find(|l| !l.trim().is_empty())?.trim();
    let (ppid, command) = line.split_once(char::is_whitespace)?;
    Some(ProcessSnapshot {
        ppid: ppid.trim().parse().ok()?,
        command: command.trim().to_string(),
    })
}

fn terminate_pid(pid: u32) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let status = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("kill exited with {}", status))
        }
    }

    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        crate::process_util::no_window_std(&mut command);
        let status = command.status().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("taskkill exited with {}", status))
        }
    }
}

fn save_snapshot(status: &KeepAwakeStatus) {
    let path = persist_path();
    if !status.active {
        let _ = std::fs::remove_file(&path);
        return;
    }
    let bytes = match serde_json::to_vec(status) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[keepawake] failed to serialize session: {}", e);
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, bytes) {
        log::warn!("[keepawake] failed to write snapshot tmp: {}", e);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        log::warn!("[keepawake] failed to rename snapshot: {}", e);
        let _ = std::fs::remove_file(&tmp);
    }
}

fn load_snapshot() -> Option<KeepAwakeStatus> {
    let bytes = std::fs::read(persist_path()).ok()?;
    serde_json::from_slice::<KeepAwakeStatus>(&bytes).ok()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn clear_if_owner(state: &KeepAwakeState, my_id: u64, app: &AppHandle) {
    let mut guard = state.lock();
    match guard.as_ref() {
        Some(s) if s.session_id == my_id => {
            *guard = None;
            drop(guard);
            let off = KeepAwakeStatus {
                active: false,
                display: false,
                expires_at: None,
                diagnostic: platform::diagnostic(false, false),
                warnings: Vec::new(),
                admin_commands: Vec::new(),
                gnome_policy: false,
                system_block: false,
            };
            save_snapshot(&off);
            crate::api::events::emit_everywhere(app, EVENT_CHANGED, &off);
        }
        _ => {
            // Another session took over — it owns the state now, don't touch.
        }
    }
}

pub fn start(
    state: &KeepAwakeState,
    app: &AppHandle,
    duration_secs: Option<u64>,
    display: Option<bool>,
    gnome_policy: Option<bool>,
    system_block: Option<bool>,
) -> Result<KeepAwakeStatus, String> {
    // view.
    stop_inner(state);

    let display = display.unwrap_or(false);
    let gnome_policy = gnome_policy.unwrap_or(false);
    let system_block = system_block.unwrap_or(false);
    if gnome_policy {
        platform::apply_user_power_policy()?;
    }
    let expires_at = duration_secs.map(|d| now_ms() + (d as i64) * 1000);
    let session_id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();

    // Spawn the platform thread FIRST so we don't record a session we can't back.
    platform::start(
        display,
        duration_secs,
        stop_rx,
        Arc::clone(state),
        app.clone(),
        session_id,
    )?;

    let session = ActiveSession {
        stop_tx,
        display,
        expires_at,
        session_id,
        gnome_policy,
        system_block,
    };
    *state.lock() = Some(session);
    let status = build_status(true, display, expires_at, gnome_policy, system_block);
    log::info!(
        "[keepawake] started: id={}, display={}, duration={:?}, gnome_policy={}, system_block={}",
        session_id,
        display,
        duration_secs,
        gnome_policy,
        system_block
    );

    save_snapshot(&status);
    crate::api::events::emit_everywhere(app, EVENT_CHANGED, &status);
    Ok(status)
}

fn build_status(
    active: bool,
    display: bool,
    expires_at: Option<i64>,
    gnome_policy: bool,
    system_block: bool,
) -> KeepAwakeStatus {
    let diagnostic = platform::diagnostic(active, display);
    let mut warnings = Vec::new();
    let mut admin_commands = Vec::new();
    if let Some(diagnostic) = diagnostic.as_ref() {
        if active && !diagnostic.runtime_inhibit_active {
            warnings.push("runtime sleep inhibitor is not visible".to_string());
        }
        if display && !diagnostic.display_inhibit_active {
            warnings.push("display/idle inhibitor is not visible".to_string());
        }
        if diagnostic.gnome_policy_safe == Some(false) {
            warnings.push(
                "GNOME current-user power policy can still suspend or blank the session"
                    .to_string(),
            );
        }
        if diagnostic.systemd_daemon_reload_needed == Some(true) {
            warnings.push(
                "systemd reports stale unit state; run sudo systemctl daemon-reload".to_string(),
            );
        }
        if diagnostic.systemd_hard_block_active == Some(false) {
            warnings.push("systemd sleep targets/services are not masked".to_string());
        }
    }
    if system_block {
        admin_commands = platform::system_block_commands();
    }

    KeepAwakeStatus {
        active,
        display,
        expires_at,
        diagnostic,
        warnings,
        admin_commands,
        gnome_policy,
        system_block,
    }
}

pub fn stop(state: &KeepAwakeState, app: &AppHandle) -> Result<(), String> {
    stop_inner(state);
    log::info!("[keepawake] stopped");
    let off = KeepAwakeStatus {
        active: false,
        display: false,
        expires_at: None,
        diagnostic: platform::diagnostic(false, false),
        warnings: Vec::new(),
        admin_commands: Vec::new(),
        gnome_policy: false,
        system_block: false,
    };
    save_snapshot(&off);
    crate::api::events::emit_everywhere(app, EVENT_CHANGED, &off);
    Ok(())
}

pub fn resume_from_disk(state: &KeepAwakeState, app: &AppHandle) {
    let Some(snap) = load_snapshot() else { return };
    if !snap.active {
        let _ = std::fs::remove_file(persist_path());
        return;
    }

    match snap.expires_at {
        None => {
            log::info!("[keepawake] resuming indefinite session from snapshot");
            let _ = start(
                state,
                app,
                None,
                Some(snap.display),
                Some(snap.gnome_policy),
                Some(snap.system_block),
            );
        }
        Some(expires_at) => {
            let remaining_ms = expires_at - now_ms();
            if remaining_ms <= 0 {
                log::info!(
                    "[keepawake] snapshot expired during downtime ({}ms ago), discarding",
                    -remaining_ms
                );
                let _ = std::fs::remove_file(persist_path());
                return;
            }
            let remaining_secs = ((remaining_ms + 999) / 1000) as u64;
            log::info!(
                "[keepawake] resuming timed session: {}s remaining",
                remaining_secs
            );
            let _ = start(
                state,
                app,
                Some(remaining_secs),
                Some(snap.display),
                Some(snap.gnome_policy),
                Some(snap.system_block),
            );
        }
    }
}

pub fn status(state: &KeepAwakeState) -> KeepAwakeStatus {
    status_with_inspector(state, inspect_process)
}

fn status_with_inspector<I>(state: &KeepAwakeState, inspect: I) -> KeepAwakeStatus
where
    I: Fn(u32) -> Option<ProcessSnapshot>,
{
    let guard = state.lock();
    match guard.as_ref() {
        Some(s) => {
            let health = active_child_health_with(inspect);
            let mut status = build_status(
                health.alive,
                health.alive && s.display,
                if health.alive { s.expires_at } else { None },
                s.gnome_policy,
                s.system_block,
            );
            if let Some(warning) = health.warning {
                status.warnings.push(warning);
            }
            status
        }
        None => build_status(false, false, None, false, false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupervisorTick {
    Idle,
    Healthy,
    Repaired,
    RepairFailed,
}

pub(crate) fn start_supervisor(state: KeepAwakeState, app: AppHandle) {
    std::thread::Builder::new()
        .name("keepawake-supervisor".into())
        .spawn(move || loop {
            std::thread::sleep(SUPERVISOR_INTERVAL);
            let _ = supervisor_tick_with(&state, inspect_process, || {
                restart_active_session(&state, &app)
            });
        })
        .expect("failed to spawn keepawake supervisor thread");
}

fn supervisor_tick_with<I, R>(state: &KeepAwakeState, inspect: I, restart: R) -> SupervisorTick
where
    I: Fn(u32) -> Option<ProcessSnapshot>,
    R: FnOnce() -> Result<(), String>,
{
    if state.lock().is_none() {
        return SupervisorTick::Idle;
    }

    let health = active_child_health_with(inspect);
    if health.alive {
        return SupervisorTick::Healthy;
    }

    log::warn!(
        "[keepawake] active session lost child assertion: {}; respawning",
        health
            .warning
            .unwrap_or_else(|| "unknown child health failure".to_string())
    );
    match restart() {
        Ok(()) => SupervisorTick::Repaired,
        Err(e) => {
            log::warn!("[keepawake] supervisor respawn failed: {}", e);
            SupervisorTick::RepairFailed
        }
    }
}

fn restart_active_session(state: &KeepAwakeState, app: &AppHandle) -> Result<(), String> {
    let Some((display, expires_at, gnome_policy, system_block)) = state
        .lock()
        .as_ref()
        .map(|s| (s.display, s.expires_at, s.gnome_policy, s.system_block))
    else {
        return Ok(());
    };

    let duration_secs = match expires_at {
        Some(expires_at) => {
            let remaining_ms = expires_at - now_ms();
            if remaining_ms <= 0 {
                return stop(state, app);
            }
            Some(((remaining_ms + 999) / 1000) as u64)
        }
        None => None,
    };

    start(
        state,
        app,
        duration_secs,
        Some(display),
        Some(gnome_policy),
        Some(system_block),
    )
    .map(|_| ())
}

#[tauri::command]
pub fn keepawake_start(
    duration_secs: Option<u64>,
    display: Option<bool>,
    gnome_policy: Option<bool>,
    system_block: Option<bool>,
    state: tauri::State<'_, KeepAwakeState>,
    app: AppHandle,
) -> Result<KeepAwakeStatus, String> {
    start(
        &state,
        &app,
        duration_secs,
        display,
        gnome_policy,
        system_block,
    )
}

#[tauri::command]
pub fn keepawake_stop(
    state: tauri::State<'_, KeepAwakeState>,
    app: AppHandle,
) -> Result<(), String> {
    stop(&state, &app)
}

#[tauri::command]
pub fn keepawake_status(state: tauri::State<'_, KeepAwakeState>) -> KeepAwakeStatus {
    status(&state)
}

fn stop_inner(state: &KeepAwakeState) {
    if let Some(session) = state.lock().take() {
        let _ = session.stop_tx.send(());
    }
}

// ── Platform implementations ────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    const ES_CONTINUOUS: u32 = 0x80000000;
    const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
    const ES_DISPLAY_REQUIRED: u32 = 0x00000002;

    unsafe extern "system" {
        safe fn SetThreadExecutionState(es_flags: u32) -> u32;
    }

    pub fn start(
        display: bool,
        duration_secs: Option<u64>,
        stop_rx: std::sync::mpsc::Receiver<()>,
        state: KeepAwakeState,
        app: AppHandle,
        session_id: u64,
    ) -> Result<(), String> {
        std::thread::Builder::new()
            .name("keepawake".into())
            .spawn(move || {
                let mut flags = ES_CONTINUOUS | ES_SYSTEM_REQUIRED;
                if display {
                    flags |= ES_DISPLAY_REQUIRED;
                }
                SetThreadExecutionState(flags);

                let stopped_early = match duration_secs {
                    Some(secs) => stop_rx
                        .recv_timeout(std::time::Duration::from_secs(secs))
                        .is_ok(),
                    None => stop_rx.recv().is_ok(),
                };

                SetThreadExecutionState(ES_CONTINUOUS);

                // On natural expiry, clear shared state only if still owned; a rapid restart may replace the owner.
                if !stopped_early {
                    clear_if_owner(&state, session_id, &app);
                }
            })
            .map(|_| ())
            .map_err(|e| format!("failed to spawn keepawake thread: {}", e))
    }

    pub fn diagnostic(_active: bool, _display: bool) -> Option<KeepAwakeDiagnostic> {
        None
    }

    pub fn apply_user_power_policy() -> Result<(), String> {
        Err("GNOME power policy mode is only supported on Linux".to_string())
    }

    pub fn system_block_commands() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const WRAPPER_MARKER: &str = "codeterm-keepawake-wrapper";

    pub fn start(
        display: bool,
        duration_secs: Option<u64>,
        stop_rx: std::sync::mpsc::Receiver<()>,
        state: KeepAwakeState,
        app: AppHandle,
        session_id: u64,
    ) -> Result<(), String> {
        // terminates it if CodeTerm disappears and launchd reparents the wrapper to PID 1.
        let (child, caffeinate_pid) = spawn_keepawake_child(display, session_id)?;
        let child_pid = child.id();
        register_child(child_pid, display, true);
        if let Some(pid) = caffeinate_pid {
            register_child(pid, display, false);
        }

        // Share the Child between the worker thread and the error cleanup path. Whichever gets there first takes it and reaps it.
        let child_slot = Arc::new(Mutex::new(Some(child)));
        let child_for_thread = Arc::clone(&child_slot);

        std::thread::Builder::new()
            .name("keepawake".into())
            .spawn(move || {
                let stopped_early = match duration_secs {
                    Some(secs) => stop_rx
                        .recv_timeout(std::time::Duration::from_secs(secs))
                        .is_ok(),
                    None => stop_rx.recv().is_ok(),
                };

                if let Some(mut c) = child_for_thread.lock().take() {
                    let _ = terminate_pid(c.id());
                    let _ = c.wait();
                }
                // (pid-file race) or it had already been reparented away from the wrapper.
                if let Some(pid) = caffeinate_pid {
                    let _ = terminate_pid(pid);
                }
                unregister_child(child_pid);
                if let Some(pid) = caffeinate_pid {
                    unregister_child(pid);
                }

                if !stopped_early {
                    clear_if_owner(&state, session_id, &app);
                }
            })
            .map(|_| ())
            .map_err(|e| {
                // the wrapper trap reaps its caffeinate; then kill the caffeinate directly as a safety net.
                if let Some(mut c) = child_slot.lock().take() {
                    let _ = terminate_pid(c.id());
                    let _ = c.wait();
                }
                if let Some(pid) = caffeinate_pid {
                    let _ = terminate_pid(pid);
                }
                unregister_child(child_pid);
                if let Some(pid) = caffeinate_pid {
                    unregister_child(pid);
                }
                format!("failed to spawn keepawake thread: {}", e)
            })
    }

    fn spawn_keepawake_child(
        display: bool,
        session_id: u64,
    ) -> Result<(std::process::Child, Option<u32>), String> {
        let pid_file = child_pid_file(session_id);
        if let Some(parent) = pid_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&pid_file);
        let script = keepawake_wrapper_script();
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(script).arg(WRAPPER_MARKER).arg(&pid_file);
        cmd.arg("-i").arg("-s");
        if display {
            cmd.arg("-d");
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("caffeinate wrapper spawn failed: {}", e))?;
        let caffeinate_pid = wait_for_caffeinate_pid(&pid_file);
        let _ = std::fs::remove_file(&pid_file);
        Ok((child, caffeinate_pid))
    }

    fn child_pid_file(session_id: u64) -> PathBuf {
        crate::paths::data_dir()
            .join("state")
            .join(format!("keepawake-{}-caffeinate.pid", session_id))
    }

    fn wait_for_caffeinate_pid(path: &std::path::Path) -> Option<u32> {
        for _ in 0..20 {
            if let Ok(s) = std::fs::read_to_string(path) {
                if let Ok(pid) = s.trim().parse::<u32>() {
                    return Some(pid);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    }

    pub(crate) fn keepawake_wrapper_script() -> &'static str {
        r#"
pid_file=$1
shift
caffeinate "$@" &
child=$!
printf '%s\n' "$child" > "$pid_file"
cleanup() {
  kill "$child" 2>/dev/null
  wait "$child" 2>/dev/null
}
trap 'cleanup; exit 0' TERM INT HUP
trap 'cleanup' EXIT
while kill -0 "$child" 2>/dev/null; do
  ppid=$(ps -o ppid= -p $$ 2>/dev/null | tr -d ' ')
  if [ "$ppid" = "1" ]; then
    cleanup
    exit 0
  fi
  sleep 2
done
wait "$child" 2>/dev/null
"#
    }

    pub fn diagnostic(_active: bool, _display: bool) -> Option<KeepAwakeDiagnostic> {
        None
    }

    pub fn apply_user_power_policy() -> Result<(), String> {
        Err("GNOME power policy mode is only supported on Linux".to_string())
    }

    pub fn system_block_commands() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
#[path = "keepawake/linux.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;

    pub fn start(
        _display: bool,
        _duration_secs: Option<u64>,
        _stop_rx: std::sync::mpsc::Receiver<()>,
        _state: KeepAwakeState,
        _app: AppHandle,
        _session_id: u64,
    ) -> Result<(), String> {
        Err("keepawake is not supported on this platform".to_string())
    }

    pub fn diagnostic(_active: bool, _display: bool) -> Option<KeepAwakeDiagnostic> {
        None
    }

    pub fn apply_user_power_policy() -> Result<(), String> {
        Err("GNOME power policy mode is only supported on Linux".to_string())
    }

    pub fn system_block_commands() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "keepawake/tests.rs"]
mod tests;
