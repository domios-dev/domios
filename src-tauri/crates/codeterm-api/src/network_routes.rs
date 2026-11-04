use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::web_access::local_ip_address;
use serde::Deserialize;

use crate::routes::AppState;
use codeterm_service_core::api_support::OptionalJson;

mod reads;

pub async fn web_link_token(
    AxumState(state): AxumState<AppState>,
    caller: Option<axum::Extension<crate::auth::CallerIdentity>>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, &'static str)> {
    use crate::auth::CallerIdentity;
    let refused = (
        StatusCode::FORBIDDEN,
        "Token-bearing web links require the owner or authenticated General Agent",
    );
    match caller.as_ref().map(|caller| &caller.0) {
        Some(CallerIdentity::Local) => {}
        Some(CallerIdentity::Tab(actor)) => state
            .svc
            .require_neighbor_general(Some(&actor.tab_id), Some(&actor.binding_nonce))
            .map_err(|_| refused)?,
        _ => return Err(refused),
    }
    let token = state
        .config
        .lock()
        .api
        .token
        .clone()
        .filter(|token| !token.is_empty())
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Web API credential unavailable",
        ))?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({"token": token})),
    ))
}

pub async fn network_info(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let key = reads::Key::new(&state.mesh, state.api_port, reads::Kind::Network);
    reads::shared()
        .read(key, move || network_snapshot(&state))
        .await
        .map(Json)
}

fn network_snapshot(state: &AppState) -> serde_json::Value {
    let transport = codeterm_service_core::mesh::network_transport::active_provider().detect();
    state.mesh.lock().set_network_transport(transport.clone());
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    // duplicating the probe logic (which previously lacked IP validation, the GUI-bridge fallback, and a timeout).
    let tailscale_ip = state.mesh.lock().tailscale_ip.clone();
    let tailscale_dns = codeterm_service_core::mesh::discovery::tailscale_self_dns_name();
    let port = state.api_port;
    let tailscale_https_url = codeterm_service_core::mesh::tailscale_serve_https_url(port);
    let web_listener = {
        let mesh = state.mesh.lock();
        codeterm_service_core::mesh::web_ui_endpoints::local_http_listener(
            mesh.local_web_ui.as_ref(),
            &mesh.ready_listeners,
            mesh.local_http_healthy,
            port,
        )
        .map(|address| address.to_string())
    };

    serde_json::json!({
        "transport": transport,
        "web_listener": web_listener,
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "tailscale_dns": tailscale_dns,
        "api_port": port,
        "api_url": format!("http://{}:{}", local_ip, port),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:{}", ip, port)),
        "tailscale_https_url": tailscale_https_url,
    })
}

/// Read-only Tailscale Serve status for the web mirror's Web Access panel.
pub async fn serve_status_handler(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let key = reads::Key::new(&state.mesh, state.api_port, reads::Kind::Serve);
    reads::shared()
        .read(key, move || {
            let build = state.mesh.lock().build_hash.clone();
            let status = codeterm_service_core::mesh::serve_setup::probe_serve_status_for_build(
                state.api_port,
                Some(build),
            );
            serde_json::to_value(status).unwrap_or(serde_json::Value::Null)
        })
        .await
        .map(Json)
}

#[derive(Deserialize, Default)]
pub struct ServeEnableBody {
    https_port: Option<u16>,
}

/// Consent-gated: enable `tailscale serve` HTTPS → this daemon. The caller (the
/// Web Access UI) is responsible for confirming with the user first.
pub async fn serve_enable_handler(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    OptionalJson(body): OptionalJson<ServeEnableBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let body = body.unwrap_or_default();
    let result = codeterm_service_core::service::web_access::enable(
        state.svc.ui.clone(),
        state.mesh.clone(),
        state.api_port,
        body.https_port,
    )
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Web Access mutation failed",
        )
    })?;
    Ok(Json(
        serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
    ))
}

/// Tear down the Serve HTTPS listener proxying to this daemon.
pub async fn serve_disable_handler(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let result = codeterm_service_core::service::web_access::disable(
        state.svc.ui.clone(),
        state.mesh.clone(),
        state.api_port,
    )
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Web Access mutation failed",
        )
    })?;
    Ok(Json(
        serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
    ))
}

#[derive(Deserialize, Default)]
pub struct KeepAwakeStartBody {
    duration_secs: Option<u64>,
    display: Option<bool>,
    gnome_policy: Option<bool>,
    system_block: Option<bool>,
}

pub async fn keepawake_status_handler(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let st = codeterm_service_core::keepawake::status(&state.keepawake);
    Ok(Json(
        serde_json::to_value(st).unwrap_or(serde_json::Value::Null),
    ))
}

pub async fn keepawake_start_handler(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    OptionalJson(body): OptionalJson<KeepAwakeStartBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let body = body.unwrap_or_default();
    let st = codeterm_service_core::keepawake::start(
        &state.keepawake,
        &state.svc.ui,
        body.duration_secs,
        body.display,
        body.gnome_policy,
        body.system_block,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(
        serde_json::to_value(st).unwrap_or(serde_json::Value::Null),
    ))
}

pub async fn keepawake_stop_handler(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::keepawake::stop(&state.keepawake, &state.svc.ui)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Web-mirror twin of `restart_app` — restarts the daemon/desktop process.
pub async fn restart_app(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let app = match codeterm_commands::application_host::require(&state.svc) {
        Ok(app) => app,
        Err(error) => {
            return (StatusCode::SERVICE_UNAVAILABLE, error.to_string()).into_response();
        }
    };
    if app.admit_restart().is_err() {
        return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "code": "native_capability_unavailable",
                    "message": "Restart requires a native application host with restart capability. This server has no restart-capable host registered; a running desktop client alone does not provide this capability. Restart this instance using its local launcher. For development, stop and start the exact named DEV instance."
                })),
            ).into_response();
    }
    tokio::task::spawn_blocking(move || {
        if let Err(error) = app.restart() {
            log::error!("[restart] {error}");
        }
    });
    Json(serde_json::json!({ "ok": true })).into_response()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "network_routes/restart_tests.rs"]
mod restart_tests;
