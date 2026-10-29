use codeterm_update::channel::Channel;
use codeterm_update::{UpdatePhase, UpdateStateHandle};
use tauri::Manager;

use crate::app_runtime::AppHandle;
use crate::application_host::{DesktopApplicationHost, ServerStopState};

#[tauri::command]
pub async fn check_for_update(
    update_state: tauri::State<'_, UpdateStateHandle>,
    force: Option<bool>,
    channel: Option<Channel>,
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
    {
        let mut us = update_state.lock();
        us.set_phase(UpdatePhase::Checking);
        us.clear_error();
    }
    let info = match codeterm_update::checker::check_for_update(&method).await {
        Ok(info) => info,
        Err(error) => {
            update_state.lock().set_error(error.clone());
            return Err(error);
        }
    };

    let val = serde_json::to_value(&info).map_err(|e| e.to_string())?;
    let mut us = update_state.lock();
    us.last_check = Some(info);
    us.last_check_time = Some(std::time::Instant::now());
    us.set_phase(UpdatePhase::Idle);
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
    let (channel, install_method) = {
        let us = update_state.lock();
        (us.channel, us.install_method.clone())
    };
    let server_stop = app
        .try_state::<ServerStopState>()
        .map(|state| state.0.clone())
        .ok_or("the server attachment is unavailable")?;
    let handle: UpdateStateHandle = update_state.inner().clone();
    let outcome = codeterm_update::single_flight::request_apply(
        codeterm_update::single_flight::global(),
        &DesktopApplicationHost {
            app,
            update_state: handle,
            server_stop,
        },
        codeterm_update::apply::ApplyOptions {
            channel,
            manifest_url: None,
            install_method,
            initiating_tab_id: None,
        },
    )
    .await
    .inspect_err(|error| {
        update_state.lock().set_error(error.clone());
    })?;
    Ok(
        serde_json::json!({ "status": outcome.discriminant(), "message": match outcome {
        codeterm_update::single_flight::UpdateRequestOutcome::AlreadyInFlight => "An update is already being applied.",
        codeterm_update::single_flight::UpdateRequestOutcome::Applied(codeterm_update::apply::ApplyResult::NoUpdate) => "No update is available.",
        _ => "The verified installer has started.",
    } }),
    )
}

#[tauri::command]
pub fn get_update_status(update_state: tauri::State<'_, UpdateStateHandle>) -> serde_json::Value {
    let status = update_state.lock().runtime_status();
    status
}
