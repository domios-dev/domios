use tauri::AppHandle;

use crate::UpdateStateHandle;

#[tauri::command]
pub async fn check_for_update(
    update_state: tauri::State<'_, UpdateStateHandle>,
    app: AppHandle,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    if !force.unwrap_or(false) {
        let us = update_state.lock();
        if let (Some(info), Some(check_time)) = (&us.last_check, us.last_check_time) {
            if check_time.elapsed() < std::time::Duration::from_secs(3600) {
                return serde_json::to_value(info).map_err(|e| e.to_string());
            }
        }
    }

    let method = update_state.lock().install_method.clone();
    {
        let mut us = update_state.lock();
        us.set_phase(crate::update::UpdatePhase::Checking);
        us.clear_error();
    }
    let info = match crate::update::checker::check_for_update(&method).await {
        Ok(info) => info,
        Err(error) => {
            update_state.lock().set_error(error.clone());
            return Err(error);
        }
    };

    if info.update_available {
        crate::api::events::emit_everywhere(&app, "update-available", &info);
    }

    let val = serde_json::to_value(&info).map_err(|e| e.to_string())?;
    let mut us = update_state.lock();
    us.last_check = Some(info);
    us.last_check_time = Some(std::time::Instant::now());
    us.set_phase(crate::update::UpdatePhase::Idle);
    Ok(val)
}

/// Answers with the same `status` discriminant the HTTP route uses, so a caller
/// can tell a refused concurrent request from "no update available".
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    update_state: tauri::State<'_, UpdateStateHandle>,
) -> Result<serde_json::Value, String> {
    let (channel, install_method) = {
        let us = update_state.lock();
        (us.channel, us.install_method.clone())
    };
    let handle: UpdateStateHandle = update_state.inner().clone();
    let outcome = crate::update::single_flight::request_apply(
        crate::update::single_flight::global(),
        &app,
        crate::update::apply::ApplyOptions {
            channel,
            manifest_url: None,
            install_method,
            initiating_tab_id: None,
        },
        handle,
    )
    .await?;
    Ok(serde_json::json!({ "status": outcome.discriminant() }))
}

#[tauri::command]
pub fn get_update_status(update_state: tauri::State<'_, UpdateStateHandle>) -> serde_json::Value {
    let status = update_state.lock().runtime_status();
    status
}
