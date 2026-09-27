pub mod lifecycle {
    pub use codeterm_pty::session::lifecycle::*;
}
pub mod scrollback {
    pub use codeterm_pty::session::scrollback::*;
}
pub(crate) mod scrollback_restore;
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
    // A refused reduction is the guard working (logged by the store), not a
    // command failure — the sidecar gate decides whether it may prune.
    let outcome = session.lock().save(&snapshot)?;
    scrollback_sync::sync_scrollback_sidecars_after_save(&app, &outcome);
    Ok(())
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
