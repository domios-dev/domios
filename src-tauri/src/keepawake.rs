use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// Monotonic session counter — every `start()` bumps this and stores the id
/// on the ActiveSession. The platform thread captures the id on spawn and
/// only clears shared state on exit if the stored session still matches —
/// otherwise a racing new session would be clobbered by an expiring thread.
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Event emitted whenever the keep-awake session changes (started, stopped,
/// expired naturally). Frontend subscribes via `listen("keepawake-changed")`.
const EVENT_CHANGED: &str = "keepawake-changed";

/// File inside the CodeTerm data dir where the last session is snapshotted
/// so we can resume across restarts.
const PERSIST_FILE: &str = "keepawake.json";
const CHILDREN_FILE: &str = "keepawake-children.json";

/// Shared state managed by Tauri.
pub type KeepAwakeState = Arc<Mutex<Option<ActiveSession>>>;

pub fn new_state() -> KeepAwakeState {
    Arc::new(Mutex::new(None))
}

pub struct ActiveSession {
    stop_tx: std::sync::mpsc::Sender<()>,
    display: bool,
    expires_at: Option<i64>,
    session_id: u64,
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
    /// Unix timestamp in ms when the session expires, or null for indefinite.
    pub expires_at: Option<i64>,
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
    let killed = cleanup_active_children_with(terminate_pid);
    if !killed.is_empty() {
        log::warn!("[keepawake] terminated active child pid(s): {:?}", killed);
    }
}

fn cleanup_active_children_with<K>(mut kill: K) -> Vec<u32>
where
    K: FnMut(u32) -> Result<(), String>,
{
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
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("taskkill exited with {}", status))
        }
    }
}

/// Snapshot a status to disk so the next launch can resume it. Inactive
/// status clears the file. Writes go through a `.tmp` + `rename` so a crash
/// mid-write can never leave a partial file.
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

/// Clear shared state from a platform expiry thread, but only if the stored
/// session still belongs to this thread. Prevents a just-expired thread from
/// clobbering a fresh session started in the tiny window between
/// `recv_timeout` returning Err and this thread acquiring the state lock.
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
            };
            save_snapshot(&off);
            let _ = app.emit(EVENT_CHANGED, &off);
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
) -> Result<KeepAwakeStatus, String> {
    // Stop any existing session first. Don't emit an intermediate "off" state — a `start` call is a single atomic transition from the frontend's
    // view.
    stop_inner(state);

    let display = display.unwrap_or(false);
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
    };
    let status = KeepAwakeStatus {
        active: true,
        display,
        expires_at,
    };
    *state.lock() = Some(session);

    log::info!(
        "[keepawake] started: id={}, display={}, duration={:?}",
        session_id,
        display,
        duration_secs
    );

    save_snapshot(&status);
    let _ = app.emit(EVENT_CHANGED, &status);
    Ok(status)
}

pub fn stop(state: &KeepAwakeState, app: &AppHandle) -> Result<(), String> {
    stop_inner(state);
    log::info!("[keepawake] stopped");
    let off = KeepAwakeStatus {
        active: false,
        display: false,
        expires_at: None,
    };
    save_snapshot(&off);
    let _ = app.emit(EVENT_CHANGED, &off);
    Ok(())
}

/// Restore a previously-persisted session on startup.
/// - indefinite → resume as indefinite
/// - timed, not yet expired → resume with remaining duration
/// - timed, already expired during downtime → clear the file, do nothing
pub fn resume_from_disk(state: &KeepAwakeState, app: &AppHandle) {
    let Some(snap) = load_snapshot() else { return };
    if !snap.active {
        let _ = std::fs::remove_file(persist_path());
        return;
    }

    match snap.expires_at {
        None => {
            log::info!("[keepawake] resuming indefinite session from snapshot");
            let _ = start(state, app, None, Some(snap.display));
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
            let _ = start(state, app, Some(remaining_secs), Some(snap.display));
        }
    }
}

pub fn status(state: &KeepAwakeState) -> KeepAwakeStatus {
    let guard = state.lock();
    match guard.as_ref() {
        Some(s) => KeepAwakeStatus {
            active: true,
            display: s.display,
            expires_at: s.expires_at,
        },
        None => KeepAwakeStatus {
            active: false,
            display: false,
            expires_at: None,
        },
    }
}

#[tauri::command]
pub fn keepawake_start(
    duration_secs: Option<u64>,
    display: Option<bool>,
    state: tauri::State<'_, KeepAwakeState>,
    app: AppHandle,
) -> Result<KeepAwakeStatus, String> {
    start(&state, &app, duration_secs, display)
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

                // Block until stop signal or timeout
                let stopped_early = match duration_secs {
                    Some(secs) => stop_rx
                        .recv_timeout(std::time::Duration::from_secs(secs))
                        .is_ok(),
                    None => stop_rx.recv().is_ok(),
                };

                // Reset execution state
                SetThreadExecutionState(ES_CONTINUOUS);

                // On natural expiry, clear shared state only if still owned; a rapid restart may replace the owner.
                if !stopped_early {
                    clear_if_owner(&state, session_id, &app);
                }
            })
            .map(|_| ())
            .map_err(|e| format!("failed to spawn keepawake thread: {}", e))
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
        // Spawn eagerly so failure surfaces as an Err before we record any session state. The shell wrapper owns the real caffeinate process and
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
                    let _ = c.kill();
                    let _ = c.wait();
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
                // Thread spawn failed after caffeinate was already running — reap it here so we don't orphan the child process.
                if let Some(mut c) = child_slot.lock().take() {
                    let _ = c.kill();
                    let _ = c.wait();
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
}

// Fallback for Linux / other — no-op
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
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
        log::warn!("[keepawake] not supported on this platform");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    fn child(pid: u32, wrapper: bool) -> KeepAwakeChild {
        KeepAwakeChild {
            pid,
            spawned_at_ms: 123,
            display: false,
            wrapper,
        }
    }

    #[test]
    fn child_journal_round_trips_and_empty_save_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHILDREN_FILE);
        let entries = vec![child(10, true), child(11, false)];

        save_children_to(&path, &entries);
        assert_eq!(load_children_from(&path), entries);

        save_children_to(&path, &[]);
        assert!(!path.exists());
    }

    #[test]
    fn reap_kills_only_orphaned_keepawake_processes() {
        let entries = vec![child(10, true), child(11, false), child(12, false)];
        let killed = Arc::new(StdMutex::new(Vec::new()));
        let killed_for_closure = Arc::clone(&killed);

        let outcome = reap_journaled_orphans(
            entries,
            |pid| match pid {
                10 => Some(ProcessSnapshot {
                    ppid: 1,
                    command: "/bin/sh codeterm-keepawake-wrapper".into(),
                }),
                11 => Some(ProcessSnapshot {
                    ppid: 10,
                    command: "caffeinate -i -s".into(),
                }),
                12 => Some(ProcessSnapshot {
                    ppid: 1,
                    command: "python something_else.py".into(),
                }),
                _ => None,
            },
            |pid| {
                killed_for_closure.lock().unwrap().push(pid);
                Ok(())
            },
        );

        assert_eq!(*killed.lock().unwrap(), vec![10]);
        assert_eq!(outcome.killed, vec![10]);
        assert_eq!(outcome.kept, vec![child(11, false)]);
        assert_eq!(outcome.skipped, vec![12]);
    }

    #[test]
    fn exit_cleanup_attempts_all_registered_children() {
        let entries = vec![child(20, true), child(21, false)];
        let killed = Arc::new(StdMutex::new(Vec::new()));
        let killed_for_closure = Arc::clone(&killed);

        let result = cleanup_child_entries(entries, |pid| {
            killed_for_closure.lock().unwrap().push(pid);
            Ok(())
        });

        assert_eq!(result, vec![20, 21]);
        assert_eq!(*killed.lock().unwrap(), vec![20, 21]);
    }

    #[test]
    fn parse_ps_snapshot_reads_ppid_and_command() {
        let snapshot = parse_ps_snapshot("    1 /bin/sh codeterm-keepawake-wrapper -i -s\n")
            .expect("snapshot parsed");

        assert_eq!(snapshot.ppid, 1);
        assert_eq!(snapshot.command, "/bin/sh codeterm-keepawake-wrapper -i -s");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_wrapper_kills_caffeinate_when_parent_disappears() {
        let script = platform::keepawake_wrapper_script();

        assert!(script.contains("caffeinate \"$@\" &"));
        assert!(script.contains("printf '%s\\n' \"$child\""));
        assert!(script.contains("trap 'cleanup; exit 0' TERM INT HUP"));
        assert!(script.contains("[ \"$ppid\" = \"1\" ]"));
        assert!(script.contains("cleanup"));
    }
}
