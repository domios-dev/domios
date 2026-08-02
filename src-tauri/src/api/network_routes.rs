use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

pub async fn network_info(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    // duplicating the probe logic (which previously lacked IP validation, the GUI-bridge fallback, and a timeout).
    let tailscale_ip = state.mesh.lock().tailscale_ip.clone();
    let tailscale_dns = crate::mesh::discovery::tailscale_self_dns_name();
    let port = state.api_port;
    let tailscale_https_url = crate::mesh::tailscale_serve_https_url(port);

    Json(serde_json::json!({
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "tailscale_dns": tailscale_dns,
        "api_port": port,
        "api_url": format!("http://{}:{}", local_ip, port),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:{}", ip, port)),
        "tailscale_https_url": tailscale_https_url,
    }))
}

/// Read-only Tailscale Serve status for the web mirror's Web Access panel.
pub async fn serve_status_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let build = state.mesh.lock().build_hash.clone();
    let mut status = crate::mesh::probe_serve_status(state.api_port);
    status.snapshot = crate::mesh::reconcile_web_serve(state.api_port, None, Some(build));
    Ok(Json(
        serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
    ))
}

#[derive(Deserialize, Default)]
pub(crate) struct ServeEnableBody {
    https_port: Option<u16>,
}

/// Consent-gated: enable `tailscale serve` HTTPS → this daemon. The caller (the
/// Web Access UI) is responsible for confirming with the user first.
pub async fn serve_enable_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    body: Option<Json<ServeEnableBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let result = crate::service::web_access::enable(
        state.svc.app_handle.clone(),
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
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let result = crate::service::web_access::disable(
        state.svc.app_handle.clone(),
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
pub(crate) struct KeepAwakeStartBody {
    duration_secs: Option<u64>,
    display: Option<bool>,
    gnome_policy: Option<bool>,
    system_block: Option<bool>,
}

pub async fn keepawake_status_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let st = crate::keepawake::status(&state.keepawake);
    Ok(Json(
        serde_json::to_value(st).unwrap_or(serde_json::Value::Null),
    ))
}

pub async fn keepawake_start_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    body: Option<Json<KeepAwakeStartBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let st = crate::keepawake::start(
        &state.keepawake,
        &state.svc.app_handle,
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
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    crate::keepawake::stop(&state.keepawake, &state.svc.app_handle)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Web-mirror twin of `restart_app` — restarts the daemon/desktop process.
pub async fn restart_app(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let app = state.svc.app_handle.clone();
    tokio::task::spawn_blocking(move || {
        crate::crash::shutdown::mark_shutting_down_for(
            crate::crash::lifecycle::ShutdownReason::Restart,
        );
        app.restart();
    });
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}
