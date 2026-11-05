use super::*;
use crate::auth::{AuthenticatedOrigin, CallerIdentity};
use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, Request},
    routing::get,
    Router,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[test]
fn sidebar_admits_verified_owner_origin_only_including_relayed_owners() {
    for identity in [CallerIdentity::Local, CallerIdentity::Relayed] {
        let headers =
            AgentHeaders::authenticated(&identity, AuthenticatedOrigin::Owner, HeaderMap::new())
                .unwrap();
        assert!(admit(&headers.actor).is_ok());
        for origin in [AuthenticatedOrigin::Mesh, AuthenticatedOrigin::Service] {
            match AgentHeaders::authenticated(&identity, origin, HeaderMap::new()) {
                Ok(headers) => {
                    assert_eq!(admit(&headers.actor).unwrap_err().0, StatusCode::FORBIDDEN)
                }
                Err(error) => assert_eq!(error.0, StatusCode::FORBIDDEN),
            }
        }
    }
    assert_eq!(
        admit(&RequestActor::Peer).unwrap_err().0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn sidebar_route_is_read_only_and_omits_account_state() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().account = serde_json::json!({"provider":"local"});
    let (key, public) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    let instance = "sidebar-relay-owner";
    {
        let mut mesh = state.mesh.lock();
        mesh.ident.identity.instance_id = instance.into();
        mesh.ident.identity.private_key = Some(key.clone());
        mesh.ident.identity.public_key = Some(public);
    }
    let app = Router::new()
        .route("/api/neighbors/sidebar", get(read))
        .with_state(state);
    let request = |method| {
        Request::builder()
            .method(method)
            .uri("/api/neighbors/sidebar")
            .header("authorization", "Bearer preview-test-token")
            .body(Body::empty())
            .unwrap()
    };
    let response = app.clone().oneshot(request("GET")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body, serde_json::json!({}));
    for (origin, expected) in [
        (AuthenticatedOrigin::Owner, StatusCode::OK),
        (AuthenticatedOrigin::Mesh, StatusCode::FORBIDDEN),
        (AuthenticatedOrigin::Service, StatusCode::FORBIDDEN),
    ] {
        let token = codeterm_service_core::mesh::crown::sign_request_with_agent(
            &key, instance, origin, None,
        )
        .unwrap();
        let signed = Request::builder()
            .uri("/api/neighbors/sidebar")
            .header("authorization", format!("Bearer {token}"))
            .header("x-codeterm-authenticated-origin", "Owner")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(signed).await.unwrap().status(),
            expected,
            "{origin:?}"
        );
    }
    assert_eq!(
        app.clone().oneshot(request("POST")).await.unwrap().status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    let unsigned = Request::builder()
        .uri("/api/neighbors/sidebar")
        .body(Body::empty())
        .unwrap();
    assert_ne!(
        app.oneshot(unsigned).await.unwrap().status(),
        StatusCode::OK
    );
}
