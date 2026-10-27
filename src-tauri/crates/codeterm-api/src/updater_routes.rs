use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};
use codeterm_update::apply::{ApplyOptions, ApplyResult};
use codeterm_update::single_flight;
use codeterm_update::InstallMethod;
use codeterm_update::UpdatePhase;

#[derive(Deserialize)]
pub struct CheckUpdateQuery {
    pub force: Option<bool>,
}

pub async fn check_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<CheckUpdateQuery>,
    bundles: Option<axum::Extension<std::sync::Arc<codeterm_frontend_bundle::FrontendBundles>>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    if super::auth::is_local_caller(&state, &headers) {
        if let Some(axum::Extension(bundles)) = bundles {
            codeterm_service_core::startup::frontend_bundles_check(bundles);
        }
    }
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

    let info = match codeterm_update::checker::check_for_update(&method).await {
        Ok(i) => i,
        Err(e) => {
            log::debug!("[updater] check failed: {}", e);
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
    let mut status = state.update_state.lock().runtime_status();
    match tokio::task::spawn_blocking(|| {
        codeterm_update::native_handoff::read_optional(&codeterm_service_core::paths::data_dir())
    })
    .await
    {
        Ok(Some(handoff)) => status["native_update"] = serde_json::json!(handoff.result),
        Ok(None) => {}
        Err(error) => log::warn!("[updater] native status unavailable: {error}"),
    }
    Ok(Json(status))
}

#[derive(Deserialize)]
pub struct InstallUpdateQuery {
    pub method: Option<String>,
    /// Override manifest URL — gated behind `dev-updater-allowlocal` feature
    /// at the manifest fetch layer (rejected in release builds for non-https
    /// schemes). Used by local end-to-end tests.
    pub manifest_url: Option<String>,
    /// Message to continue through the General Agent's restart-resume claim.
    /// Persisted before restart so the next boot folds it into that one claim.
    pub after_update: Option<String>,
    /// Tab that initiated the update — excluded from the shutdown sweep so the
    /// caller survives to hand off / relaunch (Defect 3).
    pub continue_tab: Option<String>,
}

fn validate_after_update_enabled(
    requested: bool,
    continue_after_restart: bool,
) -> Result<(), &'static str> {
    if requested && !continue_after_restart {
        Err(codeterm_service_core::service::turn_resume::DISABLED)
    } else {
        Ok(())
    }
}

fn validate_initiating_tab(
    caller_tab_id: Option<&str>,
    requested_tab_id: Option<&str>,
) -> Result<(), &'static str> {
    if requested_tab_id.is_some_and(|requested| caller_tab_id != Some(requested)) {
        Err(codeterm_service_core::service::turn_resume::NOT_CALLER_TAB)
    } else {
        Ok(())
    }
}

pub async fn install_update(
    state: AxumState<AppState>,
    headers: HeaderMap,
    query: Query<InstallUpdateQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    match install_update_inner(state, headers, query).await {
        Ok(value) => {
            if matches!(
                value.0["status"].as_str(),
                Some("blocked" | "elevation_required")
            ) {
                log::warn!(
                    "[updater] POST /api/update/install status=200 outcome={}",
                    value.0
                );
            }
            value.into_response()
        }
        Err((status, reason)) => {
            log::error!(
                "[updater] POST /api/update/install status={} reason={}",
                status.as_u16(),
                reason
            );
            if reason == "native_capability_unavailable" {
                (
            status,
            Json(serde_json::json!({
                "code": "native_capability_unavailable",
                "message": "Automatic installation requires a native application host attached to this server. The server has no application-host updater registered; a running desktop client alone does not provide this capability. Use Settings > Updates in the desktop client, or download the official installer from https://github.com/domios-dev/domios/releases/latest and run domios update install --from-file <installer-path> locally on macOS or Windows. On Linux, install the official package with the package manager."
            })),
        ).into_response()
            } else {
                (status, reason).into_response()
            }
        }
    }
}

async fn install_update_inner(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<InstallUpdateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    if q.after_update.is_some() {
        validate_after_update_enabled(true, state.svc.configuration().agent.continue_after_restart)
            .map_err(|reason| (StatusCode::BAD_REQUEST, reason))?;
    }
    let caller_tab_id = crate::tab_routes::caller_tab_header(&headers);
    if q.method.as_deref() == Some("auto") {
        validate_initiating_tab(caller_tab_id.as_deref(), q.continue_tab.as_deref())
            .map_err(|reason| (StatusCode::BAD_REQUEST, reason))?;
    }
    let install_method = state.update_state.lock().install_method.clone();
    if q.method.as_deref() == Some("auto") && install_method == InstallMethod::Deb {
        if q.after_update.is_some() {
            return Err((
                StatusCode::BAD_REQUEST,
                "after_update_requires_native_install",
            ));
        }
        let _lease = single_flight::global()
            .try_acquire()
            .ok_or((StatusCode::CONFLICT, single_flight::ALREADY_IN_FLIGHT))?;
        let staged = codeterm_update::debian::stage(q.manifest_url.as_deref())
            .await
            .map_err(|error| {
                log::error!("[updater] Debian staging failed: {error}");
                state.update_state.lock().set_error(error);
                (
                    StatusCode::BAD_GATEWAY,
                    "Debian package verification or staging failed",
                )
            })?;
        let mut update = state.update_state.lock();
        update.clear_error();
        update.set_phase(UpdatePhase::Idle);
        return Ok(Json(
            staged
                .map(|package| package.response())
                .unwrap_or_else(|| serde_json::json!({"status": "up_to_date"})),
        ));
    }
    let application_host = if q.method.as_deref() == Some("auto") {
        Some(
            codeterm_commands::application_host::require(&state.svc).map_err(|_| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "native_capability_unavailable",
                )
            })?,
        )
    } else {
        None
    };

    let lease = if application_host.is_some() {
        match single_flight::global().try_acquire() {
            Some(lease) => Some(lease),
            None => {
                return Ok(Json(serde_json::json!({
                    "status": single_flight::ALREADY_IN_FLIGHT,
                    "message": "An update is already being applied."
                })))
            }
        }
    } else {
        None
    };

    // Persist before restart — the next boot folds it into the General Agent's resume claim.
    if let Some(text) = q.after_update.as_deref() {
        let tab_id = caller_tab_id.ok_or((
            StatusCode::BAD_REQUEST,
            codeterm_service_core::service::turn_resume::CALLER_TAB_REQUIRED,
        ))?;
        let owner = state.clone();
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || {
            let state = owner;
            let sender = {
                let mux = state.svc.mux.read();
                let tab = mux
                    .get_tab(&tab_id)
                    .ok_or((StatusCode::BAD_REQUEST, "caller_tab_not_found"))?;
                codeterm_service_core::multiplexer::types::TurnResumeSender {
                    tab_id,
                    label: tab.title.clone(),
                    from_agent: Some(tab.agent_type.is_some()),
                    from_model: tab.agent_session_model.clone(),
                }
            };
            codeterm_service_core::service::post_update::stash(&text, Some(sender)).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "update_handoff_write_failed",
                )
            })
        })
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed"))??;
    }

    // Auto update checks bypass the daemon's short-check cache to avoid stale "up_to_date" right after release.
    // Keep cache useful for polling badge and manual browser fallback paths.
    if let Some(app) = application_host {
        let lease = lease.ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "update lease unavailable",
        ))?;
        let (channel, install_method) = {
            let us = state.update_state.lock();
            (us.channel, us.install_method.clone())
        };
        let manifest_url = q.manifest_url.clone();
        let continue_tab = q.continue_tab.clone();
        let update_state = state.update_state.clone();
        codeterm_update::apply::begin_apply(&update_state);
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
                let outcome =
                    single_flight::apply_with_lease(lease, app.as_ref(), opts.clone()).await;
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
                        log::error!(
                            "[updater] POST /api/update/install status=200 phase=error: {}",
                            e
                        );
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
            let _ = codeterm_util::open_url::open_url(url.to_string());
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

#[cfg(test)]
mod tests;

pub async fn availability(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    query: Result<Query<CheckUpdateQuery>, axum::extract::rejection::QueryRejection>,
    bundles: Option<axum::Extension<std::sync::Arc<codeterm_frontend_bundle::FrontendBundles>>>,
) -> Result<
    Json<codeterm_update::update_state::AvailabilitySnapshot>,
    (StatusCode, Json<serde_json::Value>),
> {
    check_auth(&state, &headers)
        .map_err(|(status, error)| availability_error(status, error.to_string()))?;
    let Query(q) =
        query.map_err(|error| availability_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    availability_response(
        codeterm_commands::update::discovery::check(
            &state.svc.ui,
            &state.update_state,
            bundles.map(|b| b.0),
            q.force.unwrap_or(false),
        )
        .await,
    )
}

fn availability_response(
    result: Result<codeterm_update::update_state::AvailabilitySnapshot, String>,
) -> Result<
    Json<codeterm_update::update_state::AvailabilitySnapshot>,
    (StatusCode, Json<serde_json::Value>),
> {
    result
        .map(Json)
        .map_err(|error| availability_error(StatusCode::BAD_GATEWAY, error))
}

fn availability_error(status: StatusCode, detail: String) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": detail })))
}

#[cfg(test)]
mod availability_tests;
