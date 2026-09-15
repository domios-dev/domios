use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::update::apply::{
    check_stage_deb_update, debian_preflight_status, gate_staged_deb_async, ApplyOptions,
    ApplyResult, DebianElevationStatus, DebianStageGate,
};
use crate::update::single_flight;
use crate::update::InstallMethod;
use crate::update::UpdatePhase;

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
            state
                .update_state
                .lock()
                .set_error(format!("check failed: {}", e));
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
    let status = state.update_state.lock().runtime_status();
    Ok(Json(status))
}

#[derive(Deserialize)]
pub struct InstallUpdateQuery {
    pub method: Option<String>,
    /// Override manifest URL — gated behind `dev-updater-allowlocal` feature
    /// at the manifest fetch layer (rejected in release builds for non-https
    /// schemes). Used by local end-to-end tests.
    pub manifest_url: Option<String>,
    /// Message to inject into the General Agent tab after the new daemon
    /// boots. Persisted to disk before restart so the next boot picks it up.
    pub after_update: Option<String>,
    /// Tab that initiated the update — excluded from the shutdown sweep so the
    /// caller survives to hand off / relaunch (Defect 3).
    pub continue_tab: Option<String>,
}

pub async fn install_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<InstallUpdateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    // Persist before restart — the new daemon picks this up on boot and injects into the General Agent tab.
    if let Some(text) = q.after_update.as_deref() {
        if let Err(e) = crate::update::post_update::stash(text) {
            log::warn!("[updater] failed to stash after_update message: {}", e);
        }
    }

    // Auto update checks bypass the daemon's short-check cache to avoid stale "up_to_date" right after release.
    // Keep cache useful for polling badge and manual browser fallback paths.
    if q.method.as_deref() == Some("auto") {
        let Some(lease) = single_flight::global().try_acquire() else {
            return Ok(Json(serde_json::json!({
                "status": single_flight::ALREADY_IN_FLIGHT,
                "message": "An update is already being applied."
            })));
        };
        let app = state.svc.app_handle.clone();
        let (channel, install_method) = {
            let us = state.update_state.lock();
            (us.channel, us.install_method.clone())
        };
        let manifest_url = q.manifest_url.clone();
        let continue_tab = q.continue_tab.clone();
        let update_state = state.update_state.clone();
        crate::update::apply::begin_apply(&update_state);
        if install_method == InstallMethod::Deb {
            let opts = ApplyOptions {
                channel,
                manifest_url,
                install_method,
                initiating_tab_id: continue_tab.clone(),
            };
            let plan = match check_stage_deb_update(&app, &opts, update_state.clone()).await {
                Ok(plan) => plan,
                Err(e) => {
                    update_state.lock().set_error(e.clone());
                    return Ok(Json(serde_json::json!({
                        "status": "blocked",
                        "message": e,
                        "install_method": "deb"
                    })));
                }
            };

            let gate = match gate_staged_deb_async(plan, debian_preflight_status).await {
                Ok(gate) => gate,
                Err(e) => {
                    update_state.lock().set_error(e.clone());
                    return Ok(Json(serde_json::json!({
                        "status": "blocked",
                        "message": e,
                        "install_method": "deb"
                    })));
                }
            };
            let (version, path) = match gate {
                DebianStageGate::NoUpdate => {
                    update_state.lock().set_phase(UpdatePhase::Idle);
                    return Ok(Json(serde_json::json!({
                        "status": "no_update",
                        "message": "No update available."
                    })));
                }
                DebianStageGate::Ready { version, path } => (version, path),
                DebianStageGate::NotReady(preflight) => match preflight.status {
                    DebianElevationStatus::Ready => unreachable!(),
                    DebianElevationStatus::ElevationRequired => {
                        let reason = preflight.reason.unwrap_or_else(|| {
                            "Debian update requires authorized elevation".to_string()
                        });
                        update_state.lock().set_error(reason.clone());
                        return Ok(Json(serde_json::json!({
                            "status": "elevation_required",
                            "message": reason,
                            "install_method": "deb"
                        })));
                    }
                    DebianElevationStatus::Blocked => {
                        let reason = preflight.reason.unwrap_or_else(|| {
                            "Debian update cannot obtain GUI/session elevation".to_string()
                        });
                        update_state.lock().set_error(reason.clone());
                        return Ok(Json(serde_json::json!({
                            "status": "blocked",
                            "message": reason,
                            "install_method": "deb"
                        })));
                    }
                },
            };
            {
                let mut us = update_state.lock();
                us.set_phase(UpdatePhase::Applying);
                us.clear_error();
            }
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let msg = format!("tokio runtime build failed: {}", e);
                        log::error!("[updater] {}", msg);
                        update_state.lock().set_error(msg);
                        return;
                    }
                };
                rt.block_on(async move {
                    match single_flight::install_staged_deb_with_lease(
                        lease,
                        &app,
                        version.clone(),
                        path,
                        continue_tab.as_deref(),
                    )
                    .await
                    {
                        Ok(()) => {
                            update_state.lock().set_phase(UpdatePhase::Success);
                            log::info!(
                                "[updater] Debian v{} installed — restart imminent",
                                version
                            );
                        }
                        Err(e) => {
                            log::error!("[updater] Debian install failed: {}", e);
                            update_state.lock().set_error(e);
                        }
                    }
                });
            });
            return Ok(Json(serde_json::json!({
                "status": "updating",
                "message": "Verified Debian package staged. Authorized installer is applying it."
            })));
        }
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let msg = format!("tokio runtime build failed: {}", e);
                    log::error!("[updater] {}", msg);
                    update_state.lock().set_error(msg);
                    return;
                }
            };
            rt.block_on(async {
                let opts = ApplyOptions {
                    channel,
                    manifest_url,
                    install_method,
                    initiating_tab_id: continue_tab,
                };
                let outcome = single_flight::apply_with_lease(
                    lease,
                    &app,
                    opts.clone(),
                    update_state.clone(),
                )
                .await;
                match outcome {
                    Ok(result) => match result {
                        ApplyResult::Restarting => {
                            update_state.lock().set_phase(UpdatePhase::Success);
                            log::info!("[updater] check_and_apply success — restart imminent");
                        }
                        ApplyResult::NoUpdate => {
                            update_state.lock().set_phase(UpdatePhase::Idle);
                            log::info!("[updater] check_and_apply: no update available");
                        }
                    },
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

    // Non-auto paths still need the resolved info (download URL, install command). Fall back on the cached check; if it's missing, ask the caller to poll /check first.
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
            let url = info.download_url.as_deref().unwrap_or(&info.release_url);
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
    #[allow(dead_code)]
    pub channel: String,
}

pub async fn set_channel(
    AxumState(_state): AxumState<AppState>,
    headers: HeaderMap,
    Json(_body): Json<SetChannelBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&_state, &headers)?;
    Err((StatusCode::CONFLICT, "update channel switching is disabled"))
}
