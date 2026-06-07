use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

pub async fn network_info(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    // Single source of truth: read the cached value populated by
    // `mesh::discovery::detect_tailscale` from the periodic refresh loop.
    // Avoids duplicating the probe logic (which previously lacked IP
    // validation, the GUI-bridge fallback, and a timeout).
    let tailscale_ip = state.mesh.lock().tailscale_ip.clone();
    let tailscale_dns = crate::mesh::discovery::tailscale_self_dns_name();
    let tailscale_https_url = if crate::mesh::discovery::tailscale_serve_https_enabled() {
        tailscale_dns.as_ref().map(|dns| format!("https://{}", dns))
    } else {
        None
    };
    let port = state.api_port;

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

#[derive(Deserialize, Default)]
pub(crate) struct KeepAwakeStartBody {
    duration_secs: Option<u64>,
    display: Option<bool>,
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

pub fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}
