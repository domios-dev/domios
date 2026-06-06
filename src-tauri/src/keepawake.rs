use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
    // Stop any existing session first. Don't emit an intermediate "off" state —
    // a `start` call is a single atomic transition from the frontend's view.
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

                // If timer expired naturally (not stopped early), clear shared
                // state — but only if we still own it. A fresh start() could
                // have replaced us in the tiny window after recv_timeout.
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

    pub fn start(
        display: bool,
        duration_secs: Option<u64>,
        stop_rx: std::sync::mpsc::Receiver<()>,
        state: KeepAwakeState,
        app: AppHandle,
        session_id: u64,
    ) -> Result<(), String> {
        // Spawn caffeinate eagerly so failure surfaces as an Err before we
        // record any session state.
        let mut cmd = std::process::Command::new("caffeinate");
        cmd.arg("-i").arg("-s"); // idle + system sleep prevention
        if display {
            cmd.arg("-d"); // display sleep prevention
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("caffeinate spawn failed: {}", e))?;

        // Share the Child between the worker thread and the error cleanup
        // path. Whichever gets there first takes it and reaps it.
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

                if !stopped_early {
                    clear_if_owner(&state, session_id, &app);
                }
            })
            .map(|_| ())
            .map_err(|e| {
                // Thread spawn failed after caffeinate was already running —
                // reap it here so we don't orphan the child process.
                if let Some(mut c) = child_slot.lock().take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                format!("failed to spawn keepawake thread: {}", e)
            })
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
