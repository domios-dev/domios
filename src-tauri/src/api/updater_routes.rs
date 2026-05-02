use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::update::apply::{check_and_apply, ApplyOptions};
use crate::update::self_update::{self, SelfUpdateOptions};
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
    /// Override manifest URL — gated behind `dev-updater-allowlocal` feature
    /// at the manifest fetch layer (rejected in release builds for non-https
    /// schemes). Used by local end-to-end tests.
    pub manifest_url: Option<String>,
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
        let channel = state.update_state.lock().channel;
        let manifest_url = q.manifest_url.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                let opts = ApplyOptions { channel, manifest_url };
                if let Err(e) = check_and_apply(&app, opts).await {
                    log::error!("[updater] check_and_apply failed: {}", e);
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

#[derive(Deserialize)]
pub struct SelfUpdateApplyQuery {
    pub channel: Option<String>,
}

pub async fn self_update_apply(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<SelfUpdateApplyQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let channel: Channel = match q.channel.as_deref() {
        Some(s) => s
            .parse()
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid channel"))?,
        None => state.update_state.lock().channel,
    };

    let app = state.svc.app_handle.clone();
    let opts = SelfUpdateOptions {
        channel,
        download_url_override: None,
    };
    let outcome = self_update::apply(&app, opts)
        .await
        .map_err(|e| {
            log::error!("[updater] self-update apply failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "self-update failed")
        })?;

    Ok(Json(serde_json::to_value(outcome).unwrap_or_default()))
}

pub async fn self_update_rollback(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let app = state.svc.app_handle.clone();
    let outcome = self_update::rollback(&app).await.map_err(|e| {
        log::error!("[updater] self-update rollback failed: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "rollback failed")
    })?;
    Ok(Json(serde_json::to_value(outcome).unwrap_or_default()))
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
