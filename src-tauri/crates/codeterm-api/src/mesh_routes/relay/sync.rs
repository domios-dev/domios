use super::*;

pub(super) fn sync_input_for_access<'a>(
    full_access: bool,
    sender_id: Option<&str>,
    peers: &'a [codeterm_service_core::mesh::types::GossipPeerInfo],
) -> Vec<&'a codeterm_service_core::mesh::types::GossipPeerInfo> {
    peers
        .iter()
        .filter(|peer| full_access || Some(peer.instance_id.as_str()) == sender_id)
        .collect()
}

pub(super) fn sync_output_for_access(
    full_access: bool,
    our_id: &str,
    mut payload: codeterm_service_core::mesh::types::MeshSyncPayload,
) -> codeterm_service_core::mesh::types::MeshSyncPayload {
    if !full_access {
        payload.peers.retain(|peer| peer.instance_id == our_id);
        payload.endpoints.clear();
    }
    payload
}

pub async fn mesh_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(payload): Json<codeterm_service_core::mesh::types::MeshSyncPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    log::debug!("[mesh/sync] Incoming sync: {} peers", payload.peers.len());

    let (full_access, sender_id) = crate::auth::off_runtime(&state, &headers, |state, headers| {
        let access = crate::auth::caller_access_level(state, headers)?;
        let full_access = access == codeterm_service_core::mesh::types::AccessLevel::Full;
        // Re-verify sender token here only to recover authoritative sender_id for peer bookkeeping.
        // If parsing fails, fail soft so new token formats don't disrupt gossip.
        let sender_id = sender_id_from_headers(state, headers);
        Ok((full_access, sender_id))
    })
    .await?;

    // Sync only refreshes metadata of already-paired peers; it never discovers new ones.
    if sender_id.is_some() {
        codeterm_service_core::mesh::endpoint_gossip::receive_endpoint_records(
            &state.mesh,
            payload.endpoints,
        )
        .await;
    }
    let mesh_state = state.mesh.clone();
    let peers = payload.peers;
    let our_payload = tokio::task::spawn_blocking(move || {
        let mut mesh = mesh_state.lock();
        if let Some(voucher) = sender_id.as_deref() {
            mesh.apply_machine_identity_sync(voucher, &peers);
            for info in sync_input_for_access(full_access, Some(voucher), &peers) {
                mesh.apply_gossip_peer_info(info);
            }
        }
        let our_id = mesh.ident.identity.instance_id.clone();
        let our_payload = sync_output_for_access(full_access, &our_id, mesh.build_sync_payload());
        drop(mesh);
        serde_json::to_value(our_payload).unwrap()
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    Ok(Json(our_payload))
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
    let (known_keys, _) = crate::auth::mesh_keys(state);
    codeterm_service_core::mesh::crown::verify_signed_request(token, &known_keys).ok()
}
