pub mod lifecycle {
    pub use codeterm_pty::session::lifecycle::*;
}
pub mod scrollback {
    pub use codeterm_pty::session::scrollback::*;
}
pub(crate) mod scrollback_sync;
pub mod store;

use std::sync::Arc;

use parking_lot::Mutex;

use crate::multiplexer::commands::MuxState;
pub use store::{SessionSnapshot, SessionStore};

pub type SessionState = Arc<Mutex<SessionStore>>;

#[tauri::command]
pub fn save_session(
    app: tauri::AppHandle,
    mux: tauri::State<'_, MuxState>,
    session: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let snapshot = mux.write().to_session_snapshot();
    let saved = session.lock().save(&snapshot);
    if saved.is_ok() {
        scrollback_sync::sync_scrollback_sidecars(&app);
    }
    saved
}

/// Bounded display-buffer tail for a restored plain-shell tab. Agent tabs get
/// `None` — provider resume owns their restoration, never a sidecar replay.
#[tauri::command]
pub fn load_scrollback_tail(
    mux: tauri::State<'_, MuxState>,
    session: tauri::State<'_, SessionState>,
    tab_id: String,
) -> Option<Vec<u8>> {
    let eligible = {
        let state = mux.read();
        state
            .get_tab(&tab_id)
            .map(crate::pty::scrollback_capture::scrollback_eligible)
            .unwrap_or(false)
    };
    if !eligible {
        return None;
    }
    session.lock().load_scrollback_tail(&tab_id)
}

#[tauri::command]
pub fn save_session_named(
    mux: tauri::State<'_, MuxState>,
    session: tauri::State<'_, SessionState>,
    name: String,
) -> Result<(), String> {
    let snapshot = mux.write().to_session_snapshot();
    session.lock().save_named(&name, &snapshot)
}

#[tauri::command]
pub fn list_saved_sessions(session: tauri::State<'_, SessionState>) -> Result<Vec<String>, String> {
    session.lock().list_saved()
}

#[tauri::command]
pub fn load_session(
    session: tauri::State<'_, SessionState>,
    name: Option<String>,
) -> Result<Option<SessionSnapshot>, String> {
    let store = session.lock();
    match name {
        Some(n) => store.load_named(&n),
        None => store.load_last(),
    }
}
