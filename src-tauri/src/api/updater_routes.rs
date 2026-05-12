use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::update::apply::{check_and_apply, ApplyOptions};
use crate::update::{Channel, UpdatePhase};

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
    {
        let mut us = state.update_state.lock();
        us.set_phase(UpdatePhase::Checking);
        us.clear_error();
    }

    let info = match crate::update::checker::check_for_update(&method).await {
        Ok(i) => i,
        Err(e) => {
            log::error!("[updater] check failed: {}", e);
            state.update_state.lock().set_error(format!("check failed: {}", e));
            return Err((StatusCode::BAD_GATEWAY, "Failed to check for updates"));
        }
    };

    let mut us = state.update_state.lock();
    us.last_check = Some(info.clone());
    us.last_check_time = Some(std::time::Instant::now());
    us.set_phase(UpdatePhase::Idle);
    drop(us);

    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

pub async fn update_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let us = state.update_state.lock();
    let elapsed_secs = us.phase_changed_at.map(|t| t.elapsed().as_secs());
    let body = serde_json::json!({
        "phase": us.phase.to_string(),
        "last_error": us.last_error.clone(),
        "channel": us.channel.to_string(),
        "install_method": us.install_method.to_string(),
        "current_version": env!("CARGO_PKG_VERSION"),
        "latest_known": us.last_check.as_ref().map(|i| i.latest_version.clone()),
        "update_available": us.last_check.as_ref().map(|i| i.update_available).unwrap_or(false),
        "phase_age_secs": elapsed_secs,
    });
    Ok(Json(body))
}

#[derive(Deserialize)]
pub struct InstallUpdateQuery {
    pub method: Option<String>,
    /// Override manifest URL — gated behind `dev-updater-allowlocal` feature
    /// at the manifest fetch layer (rejected in release builds for non-https
    /// schemes). Used by local end-to-end tests.
    pub manifest_url: Option<String>,
    /// Message to inject into the General Agent pane after the new daemon
    /// boots. Persisted to disk before restart so the next boot picks it up.
    pub after_update: Option<String>,
}

pub async fn install_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<InstallUpdateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    // Persist before restart — the new daemon picks this up on boot and
    // injects into the General Agent pane.
    if let Some(text) = q.after_update.as_deref() {
        if let Err(e) = crate::update::post_update::stash(text) {
            log::warn!("[updater] failed to stash after_update message: {}", e);
        }
    }

    // For the auto path the embedded tauri-plugin-updater always re-fetches
    // the manifest before download/install — going through it directly avoids
    // the daemon's cached `last_check` short-circuiting "up_to_date" right
    // after a release lands but before the 1h check cache expires. The cache
    // remains useful for the badge polling and the manual download/browser
    // fallback paths below.
    if q.method.as_deref() == Some("auto") {
        let app = state.svc.app_handle.clone();
        let channel = state.update_state.lock().channel;
        let manifest_url = q.manifest_url.clone();
        let update_state = state.update_state.clone();
        {
            let mut us = update_state.lock();
            us.set_phase(UpdatePhase::Checking);
            us.clear_error();
        }
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let msg = format!("tokio runtime build failed: {}", e);
                    log::error!("[updater] {}", msg);
                    update_state.lock().set_error(msg);
                    return;
                }
            };
            rt.block_on(async {
                let opts = ApplyOptions { channel, manifest_url };
                match check_and_apply(&app, opts.clone(), update_state.clone()).await {
                    Ok(applied) => {
                        if applied {
                            update_state.lock().set_phase(UpdatePhase::Success);
                            log::info!("[updater] check_and_apply success — restart imminent");
                        } else {
                            update_state.lock().set_phase(UpdatePhase::Idle);
                            log::info!("[updater] check_and_apply: no update available");
                        }
                    }
                    Err(e) => {
                        log::error!("[updater] check_and_apply failed: {}", e);
                        update_state.lock().set_error(e);
                    }
                }
            });
        });
        return Ok(Json(serde_json::json!({
            "status": "updating",
            "message": "Checking for the latest version. App will restart if an update is available."
        })));
    }

    // Non-auto paths still need the resolved info (download URL, install
    // command). Fall back on the cached check; if it's missing, ask the
    // caller to poll /check first.
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
