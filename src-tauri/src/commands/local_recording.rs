use crate::service::ServiceState;

#[tauri::command]
pub(crate) fn local_recording_status(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
) -> Result<codeterm_plugin::local_recording::RecordingStatus, String> {
    svc.local_recording_status(&plugin_id)
}

#[tauri::command]
pub(crate) fn local_recording_set_enabled(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    enabled: bool,
) -> Result<codeterm_plugin::local_recording::RecordingStatus, String> {
    svc.local_recording_set_enabled(&plugin_id, enabled)
}

#[tauri::command]
pub(crate) fn local_recording_select_devices(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    input_id: String,
    collection_id: String,
    microphone_id: String,
) -> Result<u64, String> {
    svc.local_recording_select_devices(
        &plugin_id,
        codeterm_plugin::local_recording::SelectedDevices {
            input_id,
            collection_id,
            microphone_id,
        },
    )
}

#[tauri::command]
pub(crate) fn local_recording_capture_stopped(
    svc: tauri::State<'_, ServiceState>,
    lease: String,
) -> Result<(), String> {
    svc.local_recording_capture_stopped(&lease)
}

#[tauri::command]
pub(crate) fn local_recording_cancel(svc: tauri::State<'_, ServiceState>, lease: String) {
    svc.local_recording_cancel(&lease);
}

#[tauri::command]
pub(crate) fn local_recording_manual_capture(
    svc: tauri::State<'_, ServiceState>,
    active: bool,
) -> Result<(), String> {
    svc.local_recordings.set_manual_capture(active)
}

#[tauri::command]
pub(crate) async fn local_recording_commit(
    svc: tauri::State<'_, ServiceState>,
    lease: String,
    path: String,
    client_token: String,
) -> Result<crate::service::agent_pad::ChatSendResult, String> {
    svc.local_recording_commit(&lease, std::path::Path::new(&path), client_token)
        .await
}
