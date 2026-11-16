use super::*;
use crate::auth::AuthenticatedOrigin;
use axum::{body::Body, http::Request, routing::post, Router};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn every_remote_neighbor_route_requires_verified_owner_origin() {
    let fixture = codeterm_neighbors_service::test_support::fixture(MultiplexerState::new().0);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let (key, public) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    let instance = "neighbor-view-owner";
    {
        let mut mesh = state.mesh.lock();
        mesh.ident.identity.instance_id = instance.into();
        mesh.ident.identity.private_key = Some(key.clone());
        mesh.ident.identity.public_key = Some(public);
    }
    let app = Router::new()
        .route("/api/neighbors/list", post(list))
        .route("/api/neighbors/runtime", post(runtime))
        .route("/api/neighbors/files", post(files))
        .with_state(state);
    for (path, body) in [
        ("/api/neighbors/list", json!({"operation":"list"})),
        ("/api/neighbors/runtime", json!({"operation":"status"})),
        (
            "/api/neighbors/files",
            json!({"operation":"usage","friendship_id":"a".repeat(64)}),
        ),
    ] {
        for origin in [
            AuthenticatedOrigin::Owner,
            AuthenticatedOrigin::Mesh,
            AuthenticatedOrigin::Service,
            AuthenticatedOrigin::Agent,
        ] {
            let token = codeterm_service_core::mesh::crown::sign_request_with_agent(
                &key, instance, origin, None,
            )
            .unwrap();
            let request = Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .header("x-codeterm-authenticated-origin", "Owner")
                .body(Body::from(body.to_string()))
                .unwrap();
            let status = app.clone().oneshot(request).await.unwrap().status();
            if origin == AuthenticatedOrigin::Owner {
                assert_ne!(status, StatusCode::FORBIDDEN, "owner admission: {path}");
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{origin:?}: {path}");
            }
        }
    }
}

#[tokio::test]
async fn remote_owner_writes_are_refused_before_service_work() {
    let fixture = codeterm_neighbors_service::test_support::fixture(MultiplexerState::new().0);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let app = Router::new()
        .route("/api/neighbors/list", post(list))
        .route("/api/neighbors/runtime", post(runtime))
        .route("/api/neighbors/files", post(files))
        .with_state(state);
    for (path, body) in [
        (
            "/api/neighbors/list",
            json!({"operation":"remove","friendship_id":"a".repeat(64)}),
        ),
        ("/api/neighbors/runtime", json!({"operation":"rotate"})),
        (
            "/api/neighbors/runtime",
            json!({"operation":"send","friendship_id":"a".repeat(64),"kind":"question","body":"opaque","in_reply_to":null}),
        ),
        (
            "/api/neighbors/runtime",
            json!({"operation":"set_permission","friendship_id":"a".repeat(64),"permission":{}}),
        ),
        (
            "/api/neighbors/runtime",
            json!({"operation":"set_friend_receiver","friendship_id":"a".repeat(64),"receiver_operation":"set","takeover_confirmed":true,"expected_receiver_revision":2}),
        ),
        (
            "/api/neighbors/files",
            json!({"operation":"open","friendship_id":"a".repeat(64),"file_id":"opaque","launch":false}),
        ),
        (
            "/api/neighbors/files",
            json!({"operation":"set_daily_limit","friendship_id":"a".repeat(64),"file_daily_limit_bytes":0}),
        ),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("authorization", "Bearer preview-test-token")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{path}: {body}"
        );
    }
}
