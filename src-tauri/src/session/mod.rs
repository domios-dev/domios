pub mod lifecycle {
    pub use codeterm_pty::session::lifecycle::*;
}
pub mod scrollback {
    pub use codeterm_pty::session::scrollback::*;
}
pub(crate) mod scrollback_restore;
pub(crate) mod scrollback_sync;
pub mod store;

use crate::multiplexer::commands::MuxState;
pub use store::{SessionSnapshot, SessionStore};

use std::sync::Arc;

use parking_lot::Mutex;

pub type SessionState = Arc<Mutex<SessionStore>>;

#[tauri::command]
pub async fn save_session(
    app: tauri::AppHandle,
    mux: tauri::State<'_, MuxState>,
    _session: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let app = app.clone();
    let mux = mux.inner().clone();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::service::session_persist::write_session_snapshot(&app, &mux)
    })
    .await
    .map_err(|error| format!("session persistence task failed: {error}"))??;
    if let store::SaveOutcome::RefusedReduction(refusal) = outcome {
        log::error!("[session] explicit save preserved the fuller desk: {refusal}");
    }
    Ok(())
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
