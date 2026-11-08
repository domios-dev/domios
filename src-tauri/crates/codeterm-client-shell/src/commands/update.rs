use codeterm_update::channel::Channel;
use codeterm_update::UpdateStateHandle;
use tauri::Manager;

use crate::app_runtime::AppHandle;
use crate::application_host::NativeUpdateApplyState;

#[tauri::command]
pub async fn check_for_update(
    update_state: tauri::State<'_, UpdateStateHandle>,
    force: Option<bool>,
    channel: Option<Channel>,
    retire_error_revision: Option<String>,
) -> Result<serde_json::Value, String> {
    if let Some(channel) = channel {
        update_state.lock().channel = channel;
    }
    if !force.unwrap_or(false) {
        let us = update_state.lock();
        if let (Some(info), Some(check_time)) = (&us.last_check, us.last_check_time) {
            if check_time.elapsed() < std::time::Duration::from_secs(3600) {
                return serde_json::to_value(info).map_err(|e| e.to_string());
            }
        }
    }

    let method = update_state.lock().install_method.clone();
    let trigger = codeterm_update::check_throttle::CheckTrigger::from_force(force.unwrap_or(false));
    let info = codeterm_update::checker::check_for_update(&method, trigger).await?;

    let val = serde_json::to_value(&info).map_err(|e| e.to_string())?;
    let mut us = update_state.lock();
    if !info.update_available && !codeterm_update::single_flight::global().is_in_flight() {
        us.retire_error(retire_error_revision.as_deref());
    }
    us.last_check = Some(info);
    us.last_check_time = Some(std::time::Instant::now());
    Ok(val)
}

/// Answers with the same `status` discriminant the HTTP route uses, so a caller
/// can tell a refused concurrent request from "no update available".
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    update_state: tauri::State<'_, UpdateStateHandle>,
    channel: Option<Channel>,
) -> Result<serde_json::Value, String> {
    if let Some(channel) = channel {
        update_state.lock().channel = channel;
    }
    let channel = update_state.lock().channel;
    let updater = app
        .try_state::<NativeUpdateApplyState>()
        .map(|state| state.0.clone())
        .ok_or("the native updater attachment is unavailable")?;
    updater.apply(channel).await
}

#[tauri::command]
pub fn get_update_status(update_state: tauri::State<'_, UpdateStateHandle>) -> serde_json::Value {
    let status = update_state.lock().runtime_status();
    status
}
