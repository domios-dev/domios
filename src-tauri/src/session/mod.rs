pub mod lifecycle {
    pub use codeterm_pty::session::lifecycle::*;
}
pub mod store;

use std::sync::Arc;

use parking_lot::Mutex;

use crate::multiplexer::commands::MuxState;
pub use store::{SessionSnapshot, SessionStore};

pub type SessionState = Arc<Mutex<SessionStore>>;

#[tauri::command]
pub fn save_session(
    mux: tauri::State<'_, MuxState>,
    session: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let snapshot = mux.write().to_session_snapshot();
    session.lock().save(&snapshot)
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
