//! Mesh networking API routes — discovery, pairing, relay, sync.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use crate::api::routes::{check_auth, AppState};
use crate::mesh::types::address_tier;

pub mod announce;
pub mod connection;
pub mod introduction;
pub mod pairing;
pub mod relay;

pub use announce::{mesh_announce_all, mesh_announce_inbound, mesh_refresh, mesh_set_peer_addr};
pub use connection::{
    mesh_approve_peer, mesh_approve_peer_by_id, mesh_connect_peer, mesh_disconnect_peer,
    mesh_disconnect_peer_by_id, mesh_reject_peer, mesh_reject_peer_by_id,
};
pub use introduction::mesh_introduce;
pub use pairing::{
    mesh_pair_accept, mesh_pair_confirm, mesh_pair_reject, mesh_pair_request, mesh_unpair,
};
pub use relay::{mesh_relay, mesh_sync};

pub async fn mesh_ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({"mesh": true}))
}

pub async fn mesh_debug_sign(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    match mesh.sign_request() {
        Ok(token) => Ok(Json(
            serde_json::json!({"token": token, "instance_id": mesh.ident.identity.instance_id}),
        )),
        Err(e) => Ok(Json(serde_json::json!({"error": e}))),
    }
}

pub async fn mesh_start_broadcast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let duration = state.config.lock().mesh.broadcast_duration_secs;
    crate::mesh::discovery::start_broadcast(state.mesh.clone(), duration).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to start broadcast",
        )
    })?;
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(
        serde_json::json!({"ok": true, "duration_secs": duration}),
    ))
}

pub async fn mesh_stop_broadcast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::mesh::discovery::stop_broadcast(&state.mesh);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(serde::Deserialize)]
pub struct AddManualBody {
    pub address: String,
    pub port: u16,
}

pub async fn mesh_add_manual(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AddManualBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let peer = crate::mesh::types::DiscoveredPeer {
        instance_id: format!("manual-{}", uuid::Uuid::new_v4()),
        instance_name: format!("{}:{}", body.address, body.port),
        addresses: vec![crate::mesh::types::PeerAddress::from_ip(&body.address)],
        api_port: body.port,
        discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
        last_seen: now,
        dev: false,
        mesh_id: None,
        vouched_by: Vec::new(),
    };
    let id = peer.instance_id.clone();
    state.mesh.lock().discovered_peers.insert(id.clone(), peer);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true, "instance_id": id})))
}

pub async fn mesh_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.status()).unwrap()))
}

pub async fn mesh_peers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.peers_response()).unwrap()))
}

#[derive(serde::Deserialize)]
struct PeerWebTokenResponse {
    token: String,
}

pub async fn mesh_peer_web_token(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    let peer = {
        let mesh = state.mesh.lock();
        mesh.paired_peers.get(&id).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Paired peer {} was not found", id),
            )
        })?
    };
    if peer.addresses.is_empty() {
        return Err((StatusCode::BAD_GATEWAY, "Peer has no addresses".to_string()));
    }

    let signed_token = state.mesh.lock().sign_request().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Sign failed: {}", e),
        )
    })?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let mut addresses = peer.addresses.clone();
    addresses.sort_by_key(address_tier);
    let mut errors = Vec::new();

    for addr in addresses {
        let base = format!("http://{}:{}", addr.ip, peer.api_port);
        let url = format!("{}/api/mesh/web-token", base);
        match client
            .get(&url)
            .header("Authorization", format!("Bearer {}", signed_token))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.json::<PeerWebTokenResponse>().await.map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        format!("Peer returned invalid web token response: {}", e),
                    )
                })?;
                if body.token.trim().is_empty() {
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        "Peer returned an empty web token".to_string(),
                    ));
                }
                // Prefer the peer's tailscale-serve HTTPS front: an
                // https-origin browser cannot fetch an http:// host
                // (mixed content), so the plain IP would brick the switch.
                let browser_host = peer.https_url.clone().unwrap_or(base);
                return Ok(Json(serde_json::json!({
                    "ok": true,
                    "peer_id": id,
                    "host": browser_host,
                    "token": body.token,
                })));
            }
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                errors.push(format!("{} HTTP {} {}", addr.ip, status, text));
            }
            Err(e) => errors.push(format!("{} {}", addr.ip, e)),
        }
    }

    Err((
        StatusCode::BAD_GATEWAY,
        format!("Failed to mint peer web token: {}", errors.join("; ")),
    ))
}

pub async fn mesh_web_token(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    // Return a short-lived scoped token instead of the permanent master token,
    // so that the master key is never exposed outside the daemon process.
    // If the web token leaks (logs, Referer, network), it expires within 1 hour.
    let token = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let mut cache = state.web_tokens.lock().unwrap();
        // Opportunistic cleanup: drop expired tokens to bound memory.
        cache.retain(|_, created| created.elapsed() < crate::api::routes::WEB_TOKEN_TTL);
        cache.insert(token.clone(), std::time::Instant::now());
        token
    };
    Ok(Json(serde_json::json!({ "ok": true, "token": token })))
}

pub async fn mesh_discover(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    let discovered: Vec<_> = mesh.discovered_peers.values().cloned().collect();
    Ok(Json(serde_json::to_value(discovered).unwrap()))
}
