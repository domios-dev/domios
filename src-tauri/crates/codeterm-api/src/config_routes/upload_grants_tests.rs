use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, patch},
    Extension, Router,
};
use codeterm_service_core::mesh::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::json;

use tower::ServiceExt;

async fn authenticated_origin(
    Extension(origin): Extension<crate::auth::AuthenticatedOrigin>,
) -> String {
    format!("{origin:?}")
}

#[tokio::test]
async fn browser_upload_grants_require_owner_credential_for_key_partial_and_full_config() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;
    let (peer_key, peer_public) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    state.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: "full-peer".into(),
        instance_name: "Full peer".into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key: peer_public,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    let agent = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = Router::new()
        .route("/api/config", patch(super::patch_config_api))
        .route("/api/test-origin", get(authenticated_origin))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state.clone());
    let folders = json!({"work": [fixture.temp.path().join("media")]});
    let mut full = serde_json::to_value(state.config.lock().clone()).unwrap();
    full["browser_upload_folders"] = folders.clone();
    let bodies = [
        json!({"key":"browser_upload_folders.work", "value":folders["work"]}),
        json!({"key":"browser_upload_folders", "value":folders}),
        json!({"browser_upload_folders":folders}),
        full,
    ];
    let peer = codeterm_service_core::mesh::crown::sign_request(&peer_key, "full-peer").unwrap();
    let loopback = || (state.mesh.lock().sign_request().unwrap(), "Mesh");
    let chains = [
        ("agent", vec![(agent.clone(), "Agent")]),
        ("direct Full peer", vec![(peer.clone(), "Mesh")]),
        (
            "peer relay loopback",
            vec![(peer.clone(), "Mesh"), loopback()],
        ),
        (
            "nested relay loopback",
            vec![(peer.clone(), "Mesh"), loopback(), loopback()],
        ),
        (
            "agent round-trip loopback",
            vec![(agent.clone(), "Agent"), (peer, "Mesh"), loopback()],
        ),
        ("owner", vec![("owner-secret".into(), "Owner")]),
    ];
    for (label, hops) in &chains {
        for (credential, expected_origin) in hops {
            let origin = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/test-origin")
                        .header("authorization", format!("Bearer {credential}"))
                        .header("x-codeterm-authenticated-origin", "Owner")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(origin.status(), StatusCode::OK, "{label}");
            assert_eq!(
                axum::body::to_bytes(origin.into_body(), 4096)
                    .await
                    .unwrap()
                    .as_ref(),
                expected_origin.as_bytes(),
                "{label}"
            );
        }
        let (credential, expected_origin) = hops.last().unwrap();
        if *expected_origin == "Owner" {
            continue;
        }
        for body in &bodies {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PATCH")
                        .uri("/api/config")
                        .header("authorization", format!("Bearer {credential}"))
                        .header("x-codeterm-authenticated-origin", "Owner")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{label}: {body}");
            let refusal = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert_eq!(
                refusal.as_ref(),
                b"Only the owner can change browser upload folders"
            );
            assert!(state.config.lock().browser_upload_folders.is_empty());
        }
    }
    state
        .config
        .lock()
        .browser_upload_folders
        .insert("work".into(), vec!["existing".into()]);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/config")
                .header("authorization", format!("Bearer {agent}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"browser_upload_folders":{}}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    state.config.lock().browser_upload_folders.clear();
    let body = json!({"browser_upload_folders":{"work":["relative/folder"]}});
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/config")
                .header("authorization", "Bearer owner-secret")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(state.config.lock().browser_upload_folders.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_requests_remain_responsive_while_persistence_is_held() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    let before = state.config.lock().font.size;
    let config = state.config.clone();
    let mut next = config.lock().clone();
    next.font.size = before + 1;
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let writer = tokio::task::spawn_blocking(move || {
        codeterm_service_core::config::save_config_and_apply_with(&config, next, |_| {
            entered
                .send(())
                .map_err(|_| "writer start receiver closed".to_string())?;
            held.recv()
                .map_err(|_| "writer release sender closed".to_string())?;
            Ok(())
        })
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), started)
        .await
        .unwrap()
        .unwrap();
    let app = Router::new()
        .route("/api/config", get(super::get_config_api))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state.clone());
    let reader = tokio::spawn(async move {
        app.oneshot(
            Request::builder()
                .uri("/api/config?key=font.size")
                .header("authorization", "Bearer owner-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    });
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), reader).await;
    release.send(()).unwrap();
    let applied = writer.await.unwrap().unwrap();
    let response = response
        .expect("config request must finish before persistence is released")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["value"], before);
    assert_eq!(applied.font.size, before + 1);
    assert_eq!(state.config.lock().font.size, before + 1);
}
