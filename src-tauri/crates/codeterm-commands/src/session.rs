use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::session::{SessionSnapshot, SessionState};
use tauri::Manager;

#[tauri::command]
pub async fn save_session(
    service: tauri::State<'_, codeterm_service_core::service::ServiceState>,
    mux: tauri::State<'_, MuxState>,
    _session: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let service = service.inner().clone();
    let mux = mux.inner().clone();
    let outcome = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::session_persist::write_session_snapshot(&service, &mux)
    })
    .await
    .map_err(|error| format!("session persistence task failed: {error}"))??;
    if let codeterm_service_core::session::store::SaveOutcome::RefusedReduction(refusal) = outcome {
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
    app: codeterm_service_core::app_runtime::AppHandle,
    session: tauri::State<'_, SessionState>,
    name: Option<String>,
) -> Result<Option<SessionSnapshot>, String> {
    match name {
        Some(name) => session.lock().load_named(&name),
        None => {
            let authority = app
                .try_state::<codeterm_service_core::startup::StorageAuthority>()
                .ok_or_else(|| {
                    codeterm_service_core::startup::STORAGE_UNAVAILABLE_MESSAGE.to_string()
                })?;
            codeterm_service_core::session::load_current_session_from_storage(authority.inner())
        }
    }
}
