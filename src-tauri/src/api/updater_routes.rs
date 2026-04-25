use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::update::Channel;

#[derive(Deserialize)]
pub struct CheckUpdateQuery {
    pub force: Option<bool>,
}

pub async fn check_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<CheckUpdateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let force = q.force.unwrap_or(false);

    if !force {
        let us = state.update_state.lock();
        if let (Some(ref info), Some(check_time)) = (&us.last_check, us.last_check_time) {
            if check_time.elapsed() < std::time::Duration::from_secs(3600) {
                return Ok(Json(serde_json::to_value(info).unwrap_or_default()));
            }
        }
    }

    let method = state.update_state.lock().install_method.clone();

    let info = crate::update::checker::check_for_update(&method)
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "Failed to check for updates"))?;

    let mut us = state.update_state.lock();
    us.last_check = Some(info.clone());
    us.last_check_time = Some(std::time::Instant::now());

    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct InstallUpdateQuery {
    pub method: Option<String>,
}

pub async fn install_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<InstallUpdateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let us = state.update_state.lock();
    let info = us
        .last_check
        .as_ref()
        .ok_or((StatusCode::BAD_REQUEST, "Run check first"))?
        .clone();
    drop(us);

    if !info.update_available {
        return Ok(Json(serde_json::json!({"status": "up_to_date"})));
    }

    if q.method.as_deref() == Some("auto") {
        let app = state.svc.app_handle.clone();
        let latest = info.latest_version.clone();
        std::thread::spawn(move || {
            use tauri_plugin_updater::UpdaterExt;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                let updater = match app.updater() {
                    Ok(u) => u,
                    Err(e) => {
                        log::error!("[updater] Init failed: {}", e);
                        return;
                    }
                };
                let update = match updater.check().await {
                    Ok(Some(u)) => u,
                    Ok(None) => {
                        log::info!("[updater] No update found");
                        return;
                    }
                    Err(e) => {
                        log::error!("[updater] Check failed: {}", e);
                        return;
                    }
                };
                log::info!("[updater] Downloading and installing v{}...", update.version);
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => {
                        log::info!("[updater] Installed, restarting...");
                        app.restart();
                    }
                    Err(e) => log::error!("[updater] Install failed: {}", e),
                }
            });
        });
        return Ok(Json(serde_json::json!({
            "status": "updating",
            "version": latest,
            "message": format!("Downloading and installing v{}. App will restart.", latest)
        })));
    }

    match info.update_command {
        Some(cmd) => Ok(Json(serde_json::json!({
            "status": "command",
            "command": cmd,
            "message": format!("Run: {}", cmd)
        }))),
        None => {
            let url = info
                .download_url
                .as_deref()
                .unwrap_or(&info.release_url);
            let _ = crate::commands::open_url(url.to_string());
            Ok(Json(serde_json::json!({
                "status": "browser_opened",
                "url": url,
                "message": "Opened download page in browser"
            })))
        }
    }
}

pub async fn get_channel(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let channel = state.update_state.lock().channel;
    Ok(Json(serde_json::json!({ "channel": channel.to_string() })))
}

#[derive(Deserialize)]
pub struct SetChannelBody {
    pub channel: String,
}

pub async fn set_channel(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetChannelBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let parsed: Channel = body
        .channel
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid channel"))?;

    {
        let mut cfg = state.config.lock();
        cfg.updater.channel = parsed;
        if let Err(e) = crate::config::save_config_pub(&cfg) {
            log::warn!("[updater] failed to persist channel: {}", e);
        }
    }
    state.update_state.lock().channel = parsed;

    Ok(Json(serde_json::json!({ "ok": true, "channel": parsed.to_string() })))
}
