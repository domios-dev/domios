use super::*;
use axum::{body::Body, http::Request, routing::post, Router};
use codeterm_service_core::mesh::{
    crown::generate_peer_keypair, manual::PeerIdentity, types::PairingRequest, MeshManager,
};
use std::sync::Arc;
use tower::ServiceExt;

const REMOTE_ID: &str = "2895951d-328b-4f9a-8db5-9271b4353b6e";

async fn connect_route(
    State(mesh): State<codeterm_service_core::MeshStateHandle>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    connect_from_body(mesh, body["instance_id"].as_str().unwrap().into(), &body)
        .await
        .map(|r| Json(r.as_json()))
        .map_err(super::super::rest_connect_error)
}

fn manager(name: &str, port: u16) -> MeshManager {
    let (private, public) = generate_peer_keypair();
    let mut mesh = MeshManager::in_memory(
        codeterm_service_core::mesh::types::MeshIdentity {
            instance_id: uuid::Uuid::new_v4().to_string(),
            private_key: Some(private),
            public_key: Some(public),
        },
        name.into(),
        port,
    );
    mesh.ready_listeners = vec![format!("100.64.10.2:{port}").parse().unwrap()];
    mesh
}

#[tokio::test]
async fn mesh_manual_connect_router_records_pending_owner_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut remote = manager("Windows [dev]", address.port());
    remote.ident.identity.instance_id = REMOTE_ID.into();
    let remote = Arc::new(parking_lot::Mutex::new(remote));
    let identity_mesh = remote.clone();
    let incoming_mesh = remote.clone();
    let peer_router = Router::new()
        .route(
            "/api/mesh/pair/identity",
            post(move |Json(challenge): Json<IdentityChallenge>| {
                let mesh = identity_mesh.clone();
                async move {
                    let mesh = mesh.lock();
                    let identity = PeerIdentity {
                        challenge,
                        instance_id: REMOTE_ID.into(),
                        instance_name: mesh.ident.instance_name.clone(),
                        public_key: mesh.ident.identity.public_key.clone().unwrap(),
                        dev: true,
                        addresses: vec![codeterm_service_core::mesh::types::PeerAddress::from_ip(
                            "127.0.0.1",
                        )],
                        api_port: address.port(),
                        mesh_id: None,
                    };
                    Json(
                        IdentityProof::sign(
                            identity,
                            mesh.ident.identity.private_key.as_deref().unwrap(),
                        )
                        .unwrap(),
                    )
                }
            }),
        )
        .route(
            "/api/mesh/pair/request",
            post(move |Json(request): Json<PairingRequest>| {
                let mesh = incoming_mesh.clone();
                async move {
                    let (_, _, id) = crate::mesh_routes::pairing::record_incoming_pairing_request(
                        &mut mesh.lock(),
                        request,
                    )
                    .unwrap();
                    Json(serde_json::json!({"ok": true, "instance_id": id}))
                }
            }),
        );
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, peer_router)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
            .unwrap();
    });
    let local = Arc::new(parking_lot::Mutex::new(manager("Mac [dev]", 7697)));
    let local_id = local.lock().ident.identity.instance_id.clone();
    let router = Router::new()
        .route("/api/mesh/connect", post(connect_route))
        .with_state(local.clone());
    for target in ["aaaaaaaa", "2895951d", "Windows [dev]"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/mesh/connect")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"instance_id": target, "addr": address.to_string()})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        if target == "aaaaaaaa" {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert!(remote.lock().pending_requests.is_empty());
            assert!(local.lock().discovered_peers.is_empty());
            local.lock().upsert_discovered(
                codeterm_service_core::mesh::discovered::DiscoverySighting {
                    instance_id: REMOTE_ID.into(),
                    instance_name: "stale".into(),
                    addresses: vec![codeterm_service_core::mesh::types::PeerAddress::from_ip(
                        "100.64.0.2",
                    )],
                    api_port: 1,
                    method: codeterm_service_core::mesh::types::DiscoveryMethod::Udp,
                    dev: false,
                    mesh_id: None,
                    voucher: None,
                },
                0,
            );
            continue;
        }
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result["status"], "pending");
        let request_id = result["request_id"].as_str().unwrap();
        {
            let remote = remote.lock();
            assert_eq!(remote.pending_requests.len(), 1);
            let request = &remote.pending_requests[request_id];
            assert_eq!(request.from_instance_id, local_id);
            assert_eq!(
                request.status,
                codeterm_service_core::mesh::types::PairingStatus::Pending
            );
            assert!(request.introduced_by.is_none());
            assert!(remote.paired_peers.is_empty());
        }
        let local = local.lock();
        assert!(local.paired_peers.is_empty());
        assert_eq!(
            local.discovered_peers[REMOTE_ID].discovery_method,
            codeterm_service_core::mesh::types::DiscoveryMethod::Manual
        );
        assert_eq!(
            local.pairing_targets[request_id],
            (address.ip().to_string(), address.port(), REMOTE_ID.into())
        );
    }
    stop_tx.send(()).unwrap();
    server.await.unwrap();
}
