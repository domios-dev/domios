use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

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

#[derive(Serialize, Clone)]
pub struct KeepAwakeStatus {
    pub active: bool,
    pub display: bool,
    /// Unix timestamp in ms when the session expires, or null for indefinite.
    pub expires_at: Option<i64>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tauri::command]
pub fn keepawake_start(
    duration_secs: Option<u64>,
    display: Option<bool>,
    state: tauri::State<'_, KeepAwakeState>,
) -> Result<KeepAwakeStatus, String> {
    // Stop any existing session first
    stop_inner(&state);

    let display = display.unwrap_or(false);
    let expires_at = duration_secs.map(|d| now_ms() + (d as i64) * 1000);
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();

    let state_arc = Arc::clone(&*state);
    platform::start(display, duration_secs, stop_rx, state_arc);

    let session = ActiveSession { stop_tx, display, expires_at };
    let status = KeepAwakeStatus { active: true, display, expires_at };
    *state.lock() = Some(session);

    log::info!(
        "[keepawake] started: display={}, duration={:?}",
        display,
        duration_secs
    );
    Ok(status)
}

#[tauri::command]
pub fn keepawake_stop(state: tauri::State<'_, KeepAwakeState>) -> Result<(), String> {
    stop_inner(&state);
    log::info!("[keepawake] stopped");
    Ok(())
}

#[tauri::command]
pub fn keepawake_status(state: tauri::State<'_, KeepAwakeState>) -> KeepAwakeStatus {
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

                // If timer expired naturally (not stopped early), clear shared state
                if !stopped_early {
                    *state.lock() = None;
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
    ) {
        log::warn!("[keepawake] not supported on this platform");
    }
}
