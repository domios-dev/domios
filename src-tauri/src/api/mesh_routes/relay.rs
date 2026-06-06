use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use tauri::Emitter;

use crate::api::routes::{check_auth, AppState};

pub async fn mesh_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(payload): Json<crate::mesh::types::MeshSyncPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    log::info!("[mesh/sync] Incoming sync: {} peers", payload.peers.len());
    check_auth(&state, &headers).map_err(|(s, e)| {
        log::warn!("[mesh/sync] Auth FAILED: {}", e);
        (s, e.to_string())
    })?;

    // Identify the gossip sender so we can record them as a voucher on
    // every peer they tell us about. The auth_middleware already accepted
    // the bearer token; re-verify here only to recover the sender_id —
    // that's the only authoritative way to know which paired peer is
    // talking to us. Fall through silently if it doesn't parse so we don't
    // break gossip on auth-token format changes.
    let sender_id = sender_id_from_headers(&state, &headers);

    let mut mesh = state.mesh.lock();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    for peer_info in &payload.peers {
        if peer_info.instance_id == mesh.ident.identity.instance_id {
            continue;
        }
        if mesh.paired_peers.contains_key(&peer_info.instance_id) {
            continue;
        }
        let clean_addrs = crate::mesh::types::sanitize_addresses(&peer_info.addresses);

        // Merge vouchers: keep prior vouched_by entries, add this sender,
        // dedupe + sort. Stable order keeps the UI from flicker-rerendering.
        let mut vouched_by = mesh
            .discovered_peers
            .get(&peer_info.instance_id)
            .map(|p| p.vouched_by.clone())
            .unwrap_or_default();
        if let Some(ref sid) = sender_id {
            if !vouched_by.contains(sid) {
                vouched_by.push(sid.clone());
            }
        }
        vouched_by.sort();
        vouched_by.dedup();

        mesh.discovered_peers.insert(
            peer_info.instance_id.clone(),
            crate::mesh::types::DiscoveredPeer {
                instance_id: peer_info.instance_id.clone(),
                instance_name: peer_info.instance_name.clone(),
                addresses: clean_addrs,
                api_port: peer_info.api_port,
                discovery_method: crate::mesh::types::DiscoveryMethod::Gossip,
                last_seen: now,
                dev: peer_info.dev,
                mesh_id: None,
                vouched_by,
            },
        );
        log::info!(
            "[mesh/gossip] Learned about peer {} ({})",
            peer_info.instance_name,
            peer_info.instance_id
        );
    }
    drop(mesh);

    let mesh = state.mesh.lock();
    let our_payload = mesh.build_sync_payload();
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::to_value(our_payload).unwrap()))
}

/// Decode the Authorization bearer token and return the signed sender's
/// instance_id, or None if the header is absent / unsigned / unverifiable.
/// Used by mesh_sync to attribute gossip vouches; auth_middleware has
/// already gated whether the request runs at all.
fn sender_id_from_headers(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return None;
    }
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.ident.identity.public_key {
        known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
    }
    crate::mesh::crown::verify_signed_request(token, &known_keys).ok()
}

/// Sensitive relay paths that mutate the peer (install update, change channel).
/// Read-only paths (GET checks, GET channel) are not gated and remain available
/// to any paired peer.
fn path_requires_full_access(method: &str, path: &str) -> bool {
    let m = method.to_ascii_uppercase();
    let bare = path.split('?').next().unwrap_or(path);
    // DELETE /api/mesh/peers/<id> needs starts_with check — matches! can't do that
    if m == "DELETE" && bare.starts_with("/api/mesh/peers/") {
        return true;
    }
    matches!(
        (m.as_str(), bare),
        // Existing: update/install paths
        ("POST", "/api/update/install")
            | ("POST", "/api/update/channel")
            | ("POST", "/api/mesh/refresh")
        // Mesh pairing — can't let a partial peer pair new nodes
            | ("POST", "/api/mesh/pair/request")
            | ("POST", "/api/mesh/pair/accept")
            | ("POST", "/api/mesh/introduce")
        // Agent lifecycle — spawning agents is privileged
            | ("POST", "/api/agents/spawn")
        // Hook injection — hooks can approve tool calls
            | ("POST", "/api/hooks/permission")
            | ("POST", "/api/hooks/stop")
            | ("POST", "/api/hooks/session_start")
        // Config mutation
            | ("POST", "/api/config")
            | ("PUT", "/api/config")
    )
}

/// Verify relay sender: must be paired. Paired = trusted.
fn check_mesh_relay_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let (known_keys, our_id) = {
        let mesh = state.mesh.lock();
        let mut known_keys = mesh.known_public_keys();
        if let Some(ref pk) = mesh.ident.identity.public_key {
            known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
        }
        (known_keys, mesh.ident.identity.instance_id.clone())
    };
    let peer_id = crate::mesh::crown::verify_signed_request(token, &known_keys).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "Invalid or unknown signature".to_string(),
        )
    })?;
    if peer_id == our_id {
        return Ok("local".to_string());
    }

    let mut mesh = state.mesh.lock();
    if !mesh.paired_peers.contains_key(&peer_id) {
        return Err((
            StatusCode::UNAUTHORIZED,
            format!("Peer {} not paired", peer_id),
        ));
    }
    mesh.check_and_record_nonce(token)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    Ok(peer_id)
}

pub async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<crate::mesh::types::RelayCommand>,
) -> Result<Json<crate::mesh::types::RelayResponse>, (StatusCode, String)> {
    let sender_id = check_mesh_relay_auth(&state, &headers)?;
    log::info!(
        "[mesh/relay] {} {} from {}",
        cmd.method,
        cmd.path,
        sender_id
    );

    if path_requires_full_access(&cmd.method, &cmd.path) && sender_id != "local" {
        let access = state
            .mesh
            .lock()
            .paired_peers
            .get(&sender_id)
            .map(|p| p.access_level.clone())
            .unwrap_or(crate::mesh::types::AccessLevel::Full);
        if access != crate::mesh::types::AccessLevel::Full {
            return Ok(Json(crate::mesh::types::RelayResponse {
                ok: false,
                status: 403,
                data: None,
                error: Some(format!(
                    "Peer not authorized to {} {} (access_level={:?})",
                    cmd.method, cmd.path, access
                )),
            }));
        }
    }

    let sender_name = {
        let mesh = state.mesh.lock();
        mesh.paired_peers
            .get(&sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| sender_id.clone())
    };

    // Prefix text with [Mesh: PEER pane=ID] for send endpoints
    let mut body = cmd.body.clone();
    if cmd.path.contains("/send") && cmd.method.eq_ignore_ascii_case("POST") {
        if let Some(text) = body.get("text").and_then(|v| v.as_str()) {
            let source_pane = body
                .get("source_pane")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            body["text"] = serde_json::json!(format!(
                "[Mesh: {} pane={}] {}",
                sender_name, source_pane, text
            ));
            if let Some(obj) = body.as_object_mut() {
                obj.remove("source_pane");
            }
        }
    }

    let local_url = format!("http://127.0.0.1:{}{}", state.api_port, cmd.path);
    let signed_token = state.mesh.lock().sign_request().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Sign failed: {}", e),
        )
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let req = match cmd.method.to_uppercase().as_str() {
        "GET" => client.get(&local_url),
        "POST" => client.post(&local_url).json(&body),
        "PUT" => client.put(&local_url).json(&body),
        "PATCH" => client.patch(&local_url).json(&body),
        "DELETE" => client.delete(&local_url),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unsupported method: {}", cmd.method),
            ))
        }
    };

    let resp = req
        .header("Authorization", format!("Bearer {}", signed_token))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Local relay failed: {}", e),
            )
        })?;

    let status = resp.status().as_u16();
    let data: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!(null));

    if status < 400 && cmd.path.contains("/send") && cmd.method.eq_ignore_ascii_case("POST") {
        let target_pane = cmd.path.split('/').nth(3).unwrap_or("unknown").to_string();
        let preview = cmd
            .body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let writer =
            crate::services::activity::ActivityWriter::new(state.svc.services_store.conn());
        let _ = writer.record(crate::services::activity::ActivityEvent::peer_relay(
            &sender_id,
            &sender_name,
            &target_pane,
            &preview,
        ));
        let _ = state.svc.app_handle.emit(
            "service-pushed",
            serde_json::json!({"source_id": sender_name, "target": target_pane, "kind": "mesh"}),
        );
    }

    Ok(Json(crate::mesh::types::RelayResponse {
        ok: status < 400,
        status,
        data: Some(data),
        error: if status >= 400 {
            Some(format!("HTTP {}", status))
        } else {
            None
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::path_requires_full_access;

    #[test]
    fn install_update_is_gated() {
        assert!(path_requires_full_access("POST", "/api/update/install"));
        assert!(path_requires_full_access(
            "POST",
            "/api/update/install?method=auto"
        ));
        assert!(path_requires_full_access(
            "post",
            "/api/update/install?method=auto&manifest_url=foo"
        ));
    }

    #[test]
    fn set_channel_is_gated() {
        assert!(path_requires_full_access("POST", "/api/update/channel"));
    }

    #[test]
    fn check_and_get_channel_are_open() {
        assert!(!path_requires_full_access("GET", "/api/update/check"));
        assert!(!path_requires_full_access(
            "GET",
            "/api/update/check?force=true"
        ));
        assert!(!path_requires_full_access("GET", "/api/update/channel"));
    }

    #[test]
    fn unrelated_paths_are_open() {
        assert!(!path_requires_full_access(
            "POST",
            "/api/panes/general/send"
        ));
        assert!(!path_requires_full_access("GET", "/api/mesh/peers"));
    }

    #[test]
    fn mesh_refresh_is_gated() {
        assert!(path_requires_full_access("POST", "/api/mesh/refresh"));
        assert!(!path_requires_full_access("GET", "/api/mesh/refresh"));
    }

    #[test]
    fn mesh_announce_is_open() {
        // Inbound announce doesn't go through the access-level gate — auth
        // already requires a paired peer's signed token, and the apply step
        // verifies sender_id matches body.instance_id.
        assert!(!path_requires_full_access("POST", "/api/mesh/announce"));
    }

    #[test]
    fn mesh_pairing_endpoints_are_gated() {
        assert!(path_requires_full_access("POST", "/api/mesh/pair/request"));
        assert!(path_requires_full_access("POST", "/api/mesh/pair/accept"));
        assert!(path_requires_full_access("POST", "/api/mesh/introduce"));
    }

    #[test]
    fn agent_spawn_is_gated() {
        assert!(path_requires_full_access("POST", "/api/agents/spawn"));
    }

    #[test]
    fn hook_endpoints_are_gated() {
        assert!(path_requires_full_access("POST", "/api/hooks/permission"));
        assert!(path_requires_full_access("POST", "/api/hooks/stop"));
        assert!(path_requires_full_access(
            "POST",
            "/api/hooks/session_start"
        ));
    }

    #[test]
    fn config_mutation_is_gated() {
        assert!(path_requires_full_access("POST", "/api/config"));
        assert!(path_requires_full_access("PUT", "/api/config"));
        assert!(!path_requires_full_access("GET", "/api/config"));
    }

    #[test]
    fn peer_delete_is_gated() {
        assert!(path_requires_full_access(
            "DELETE",
            "/api/mesh/peers/some-uuid"
        ));
        assert!(path_requires_full_access(
            "DELETE",
            "/api/mesh/peers/peer-abc-123"
        ));
        assert!(!path_requires_full_access(
            "GET",
            "/api/mesh/peers/some-uuid"
        ));
    }
}
