use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

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

/// Snapshot a status to disk so the next launch can resume it.
/// Inactive status clears the file.
fn save_snapshot(status: &KeepAwakeStatus) {
    let path = persist_path();
    if !status.active {
        let _ = std::fs::remove_file(&path);
        return;
    }
    match serde_json::to_vec(status) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("[keepawake] failed to persist session: {}", e);
            }
        }
        Err(e) => log::warn!("[keepawake] failed to serialize session: {}", e),
    }
}

fn load_snapshot() -> Option<KeepAwakeStatus> {
    let bytes = std::fs::read(persist_path()).ok()?;
    serde_json::from_slice::<KeepAwakeStatus>(&bytes).ok()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
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
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();

    let state_arc = Arc::clone(state);
    let app_for_expiry = app.clone();
    platform::start(display, duration_secs, stop_rx, state_arc, app_for_expiry);

    let session = ActiveSession { stop_tx, display, expires_at };
    let status = KeepAwakeStatus { active: true, display, expires_at };
    *state.lock() = Some(session);

    log::info!(
        "[keepawake] started: display={}, duration={:?}",
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
    let off = KeepAwakeStatus { active: false, display: false, expires_at: None };
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
                let _ = app.emit(
                    EVENT_CHANGED,
                    &KeepAwakeStatus { active: false, display: false, expires_at: None },
                );
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
    ) {
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
                // state and notify the frontend so the tray updates in real time.
                if !stopped_early {
                    *state.lock() = None;
                    let off = KeepAwakeStatus { active: false, display: false, expires_at: None };
                    save_snapshot(&off);
                    let _ = app.emit(EVENT_CHANGED, &off);
                }
            })
            .expect("failed to spawn keepawake thread");
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
    ) {
        std::thread::Builder::new()
            .name("keepawake".into())
            .spawn(move || {
                let mut cmd = std::process::Command::new("caffeinate");
                cmd.arg("-i").arg("-s"); // idle + system sleep prevention
                if display {
                    cmd.arg("-d"); // display sleep prevention
                }

                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("[keepawake] caffeinate spawn failed: {}", e);
                        *state.lock() = None;
                        let off = KeepAwakeStatus { active: false, display: false, expires_at: None };
                        save_snapshot(&off);
                        let _ = app.emit(EVENT_CHANGED, &off);
                        return;
                    }
                };

                let stopped_early = match duration_secs {
                    Some(secs) => stop_rx
                        .recv_timeout(std::time::Duration::from_secs(secs))
                        .is_ok(),
                    None => stop_rx.recv().is_ok(),
                };

                let _ = child.kill();
                let _ = child.wait();

                if !stopped_early {
                    *state.lock() = None;
                    let off = KeepAwakeStatus { active: false, display: false, expires_at: None };
                    save_snapshot(&off);
                    let _ = app.emit(EVENT_CHANGED, &off);
                }
            })
            .expect("failed to spawn keepawake thread");
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
    ) {
        log::warn!("[keepawake] not supported on this platform");
    }
}
