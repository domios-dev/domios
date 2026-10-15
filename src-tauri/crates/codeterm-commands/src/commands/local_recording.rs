use codeterm_service_core::service::ServiceState;

#[tauri::command]
pub async fn local_recording_input_devices(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
) -> Result<Vec<codeterm_plugin::local_recording::RecordingInputDevice>, String> {
    svc.local_recording_input_devices(&plugin_id).await
}

#[tauri::command]
pub fn local_recording_status(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
) -> Result<codeterm_plugin::local_recording::RecordingStatus, String> {
    svc.local_recording_status(&plugin_id)
}

#[tauri::command]
pub fn local_recording_set_enabled(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    enabled: bool,
) -> Result<codeterm_plugin::local_recording::RecordingStatus, String> {
    svc.local_recording_set_enabled(&plugin_id, enabled)
}

#[tauri::command]
pub async fn local_recording_select_devices(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    input_id: String,
    collection_id: String,
    microphone_id: String,
) -> Result<u64, String> {
    let service = svc.inner().clone();
    tokio::task::spawn_blocking(move || {
        service.local_recording_select_devices(
            &plugin_id,
            codeterm_plugin::local_recording::SelectedDevices {
                input_id,
                collection_id,
                microphone_id,
            },
        )
    })
    .await
    .map_err(|_| "recording_selection_worker_failed".to_string())?
}

#[tauri::command]
pub fn local_recording_capture_stopped(
    svc: tauri::State<'_, ServiceState>,
    lease: String,
) -> Result<(), String> {
    svc.local_recording_capture_stopped(&lease)
}

#[tauri::command]
pub fn local_recording_cancel(svc: tauri::State<'_, ServiceState>, lease: String) {
    svc.local_recording_cancel(&lease);
}

#[tauri::command]
pub fn local_recording_manual_capture(
    svc: tauri::State<'_, ServiceState>,
    active: bool,
) -> Result<(), String> {
    svc.local_recordings.set_manual_capture(active)
}

#[tauri::command]
pub async fn local_recording_save_clip(
    svc: tauri::State<'_, ServiceState>,
    lease: String,
    data: String,
    extension: String,
) -> Result<codeterm_service_core::service::CaptureClipDescriptor, String> {
    svc.local_recording_save_clip(&lease, data, extension).await
}

#[tauri::command]
pub fn local_recording_discard_clip(
    svc: tauri::State<'_, ServiceState>,
    lease: String,
    clip_handle: String,
) {
    svc.local_recording_discard_clip(&lease, &clip_handle);
}

#[tauri::command]
pub async fn local_recording_commit(
    svc: tauri::State<'_, ServiceState>,
    lease: String,
    clip_handle: String,
    client_token: String,
) -> Result<codeterm_service_core::service::LocalRecordingCommitOutcome, String> {
    Ok(svc
        .local_recording_commit(&lease, &clip_handle, client_token)
        .await)
}
