use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::AppState;

fn record_introduction(
    mesh: &mut codeterm_service_core::mesh::MeshManager,
    intro: &codeterm_service_core::mesh::types::PeerIntroduction,
    sender_id: &str,
    timestamp: u64,
) -> bool {
    if mesh.pending_requests.values().any(|pending| {
        pending.from_instance_id == intro.instance_id
            && pending.status == codeterm_service_core::mesh::types::PairingStatus::Pending
    }) {
        return false;
    }

    let addresses = codeterm_service_core::mesh::types::sanitize_addresses(&intro.addresses);
    mesh.upsert_discovered(
        codeterm_service_core::mesh::discovered::DiscoverySighting {
            instance_id: intro.instance_id.clone(),
            instance_name: intro.instance_name.clone(),
            addresses: addresses.clone(),
            api_port: intro.api_port,
            method: codeterm_service_core::mesh::types::DiscoveryMethod::Gossip,
            dev: false,
            mesh_id: None,
            voucher: Some(sender_id.into()),
        },
        timestamp,
    );
    let request = codeterm_service_core::mesh::types::PairingRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        from_instance_id: intro.instance_id.clone(),
        from_instance_name: intro.instance_name.clone(),
        from_address: addresses
            .first()
            .map(|address| address.ip.clone())
            .unwrap_or_default(),
        from_port: intro.api_port,
        from_addresses: addresses,
        public_key: intro.public_key.clone(),
        timestamp,
        status: codeterm_service_core::mesh::types::PairingStatus::Pending,
        introduced_by: Some(sender_id.into()),
    };
    mesh.pending_requests
        .insert(request.request_id.clone(), request);
    let _ = mesh.save();
    true
}

pub async fn mesh_introduce(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(intro): Json<codeterm_service_core::mesh::types::PeerIntroduction>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.ident.identity.public_key {
        known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
    }
    let our_id = mesh.ident.identity.instance_id.clone();
    drop(mesh);

    let sender_id =
        match codeterm_service_core::mesh::crown::verify_signed_request(token, &known_keys) {
            Ok(id) => id,
            Err(e) => {
                log::warn!(
                    "[mesh/introduce] Auth failed for introduction of {}: {} (known_keys: {:?})",
                    intro.instance_name,
                    e,
                    known_keys.keys().collect::<Vec<_>>()
                );
                return Err((StatusCode::UNAUTHORIZED, format!("Unauthorized: {}", e)));
            }
        };

    {
        let mut mesh = state.mesh.lock();
        if let Err(e) = mesh.check_and_record_nonce(token) {
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    }

    if intro.instance_id == our_id {
        return Ok(Json(serde_json::json!({"ok": true, "skipped": "self"})));
    }
    if state
        .mesh
        .lock()
        .paired_peers
        .contains_key(&intro.instance_id)
    {
        return Ok(Json(
            serde_json::json!({"ok": true, "skipped": "already_paired"}),
        ));
    }
    {
        let mesh = state.mesh.lock();
        if mesh.is_blocked(&intro.instance_id) {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Peer {} is blocked", intro.instance_id),
            ));
        }
    }

    {
        let mesh = state.mesh.lock();
        if !mesh.paired_peers.contains_key(&sender_id) {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "Only paired peers can introduce. {} is not paired",
                    sender_id
                ),
            ));
        }
        if mesh.paired_peers[&sender_id].access_level
            != codeterm_service_core::mesh::types::AccessLevel::Full
        {
            return Err((
                StatusCode::FORBIDDEN,
                "Only Full peers may introduce peers".into(),
            ));
        }
    }

    log::info!(
        "[mesh/introduce] {} introduced by {} — adding to pending (consent required)",
        intro.instance_name,
        sender_id
    );

    let request_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let should_notify = {
        let mut mesh = state.mesh.lock();
        record_introduction(&mut mesh, &intro, &sender_id, request_timestamp)
    };

    if should_notify {
        state
            .svc
            .ui
            .emit("mesh-state-changed", serde_json::Value::Null);
        state.svc.log_system_notice(
            "Mesh pairing request".into(),
            Some(format!(
                "{} was introduced by {} and is waiting for owner approval.",
                intro.instance_name, sender_id
            )),
            Some("info"),
        );
    }

    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "pending": true,
        "instance_id": mesh.ident.identity.instance_id,
        "instance_name": mesh.ident.instance_name,
        "public_key": mesh.ident.identity.public_key,
        "api_port": mesh.api_port,
        "local_ip": mesh.local_ip,
    })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn introduction_creates_pending_not_paired() {
        let mut mesh = codeterm_service_core::mesh::MeshManager::new("receiver".into(), 7685);
        mesh.disk_persist = false;
        let intro = codeterm_service_core::mesh::types::PeerIntroduction {
            instance_id: "peer-bob".into(),
            instance_name: "Bob".into(),
            addresses: vec![codeterm_service_core::mesh::types::PeerAddress::from_ip(
                "100.64.0.2",
            )],
            api_port: 7685,
            public_key: "bob-key".into(),
            introduced_by: "peer-alice".into(),
        };

        assert!(super::record_introduction(
            &mut mesh,
            &intro,
            "peer-alice",
            1000
        ));

        let request = mesh.pending_requests.values().next().unwrap();
        assert_eq!(request.from_instance_id, "peer-bob");
        assert_eq!(request.introduced_by.as_deref(), Some("peer-alice"));
        assert_eq!(
            request.status,
            codeterm_service_core::mesh::types::PairingStatus::Pending
        );
        assert!(!mesh.paired_peers.contains_key("peer-bob"));
        assert!(mesh.discovered_peers["peer-bob"]
            .vouched_by
            .contains(&"peer-alice".to_string()));
    }
}
