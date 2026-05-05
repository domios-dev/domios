use tauri::{AppHandle, Emitter};

use crate::UpdateStateHandle;

#[tauri::command]
pub async fn check_for_update(
    update_state: tauri::State<'_, UpdateStateHandle>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let method = update_state.lock().install_method.clone();
    let info = crate::update::checker::check_for_update(&method)
        .await
        .map_err(|e| e.to_string())?;

    if info.update_available {
        let _ = app.emit("update-available", serde_json::to_value(&info).unwrap_or_default());
    }

    let val = serde_json::to_value(&info).map_err(|e| e.to_string())?;
    let mut us = update_state.lock();
    us.last_check = Some(info);
    us.last_check_time = Some(std::time::Instant::now());
    Ok(val)
}

#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    update_state: tauri::State<'_, UpdateStateHandle>,
) -> Result<(), String> {
    let channel = update_state.lock().channel;
    let handle: UpdateStateHandle = update_state.inner().clone();
    crate::update::apply::check_and_apply(
        &app,
        crate::update::apply::ApplyOptions {
            channel,
            manifest_url: None,
        },
        handle,
    )
    .await
    .map(|_| ())
}
