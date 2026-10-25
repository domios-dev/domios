use super::*;
use axum::{body::Body, routing::get, Router};
use codeterm_service_core::{
    mesh::{
        account_mesh::{AccountMeshState, AccountPresence},
        types::PairedPeer,
    },
    multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};
use futures_util::StreamExt;
use tower::ServiceExt;

fn request(path: &str, address: &str, token: &str) -> Request {
    let mut request = Request::builder()
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("x-forwarded-for", "100.100.100.100")
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(address.parse::<SocketAddr>().unwrap()));
    request
}

fn apply(state: &AppState, presence: AccountPresence) {
    state.mesh.lock().apply_account_state(AccountMeshState {
        presence,
        ..Default::default()
    });
}

fn signed_peer(state: &AppState) -> String {
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    state.mesh.lock().insert_legacy_pair_for_test(PairedPeer {
        instance_id: "paired".into(),
        instance_name: "paired".into(),
        addresses: vec![],
        api_port: 7685,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    codeterm_service_core::mesh::crown::sign_request(&private_key, "paired").unwrap()
}

fn router(state: &AppState) -> Router {
    Router::new()
        .route("/api/config", get(|| async { "allowed" }))
        .route("/api/health", get(|| async { "healthy" }))
        .route("/frontend/index.html", get(|| async { "frontend" }))
        .route("/api/mesh/relay", get(|| async { "custom" }))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            remote::middleware,
        ))
}

#[tokio::test]
async fn presence_address_and_credential_matrix() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let peer = signed_peer(&state);
    for presence in [
        AccountPresence::NeverSignedIn,
        AccountPresence::SignedIn {
            account_id: "account".into(),
        },
        AccountPresence::SignedOut,
    ] {
        let signed_out = presence == AccountPresence::SignedOut;
        apply(&state, presence);
        for address in [
            "127.0.0.1:9000",
            "[::1]:9000",
            "[::ffff:127.0.0.1]:9000",
            "100.64.0.2:9000",
            "192.168.1.2:9000",
        ] {
            let remote = !address
                .parse::<SocketAddr>()
                .unwrap()
                .ip()
                .to_canonical()
                .is_loopback();
            for token in ["preview-test-token", peer.as_str()] {
                let response = router(&state)
                    .oneshot(request("/api/config", address, token))
                    .await
                    .unwrap();
                let expected = if signed_out && remote {
                    StatusCode::SERVICE_UNAVAILABLE
                } else if signed_out && token == peer {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::OK
                };
                assert_eq!(
                    response.status(),
                    expected,
                    "{address}, signed_out={signed_out}, peer={}",
                    token == peer
                );
            }
        }
    }
}

#[tokio::test]
async fn signed_out_remote_routes_stay_closed_except_tailnet_web_access() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    apply(&state, AccountPresence::SignedOut);
    for path in [
        "/api/health",
        "/frontend/index.html",
        "/api/mesh/relay",
        "/api/config",
    ] {
        for peer_listener in [false, true] {
            let mut req = request(path, "100.64.0.2:9000", "preview-test-token");
            if peer_listener {
                req.extensions_mut()
                    .insert(codeterm_service_core::service::account_mesh::tailnet::TailnetListener);
            }
            assert_eq!(
                router(&state).oneshot(req).await.unwrap().status(),
                if peer_listener && path != "/api/mesh/relay" {
                    StatusCode::OK
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            );
        }
        assert_eq!(
            router(&state)
                .oneshot(request(path, "127.0.0.1:9000", "preview-test-token"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn remote_access_waits_for_persisted_account_state() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.mesh.lock().await_account_state();
    assert_eq!(
        router(&state)
            .oneshot(request(
                "/api/config",
                "192.168.1.2:9000",
                "preview-test-token"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        router(&state)
            .oneshot(request(
                "/api/config",
                "127.0.0.1:9000",
                "preview-test-token"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn signed_out_closes_open_remote_bodies_but_preserves_loopback() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let peer = signed_peer(&state);
    for token in ["preview-test-token", peer.as_str()] {
        for address in ["100.64.0.2:9000", "127.0.0.1:9000"] {
            apply(&state, AccountPresence::NeverSignedIn);
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<String, std::io::Error>>();
            let rx = std::sync::Arc::new(parking_lot::Mutex::new(Some(rx)));
            let streaming = Router::new()
                .route(
                    "/api/stream",
                    get(move || {
                        let rx = rx.lock().take().unwrap();
                        async move {
                            Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async {
                                rx.recv().await.map(|item| (item, rx))
                            }))
                        }
                    }),
                )
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                ))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    remote::middleware,
                ));
            let response = streaming
                .oneshot(request("/api/stream", address, token))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let mut stream = response.into_body().into_data_stream();
            tx.send(Ok("data: before\n\n".into())).unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), "data: before\n\n");
            apply(&state, AccountPresence::SignedOut);
            if !address.starts_with("127.") {
                apply(&state, AccountPresence::NeverSignedIn);
            } else if token == peer {
                assert!(
                    tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                        .await
                        .unwrap()
                        .is_none()
                );
                continue;
            }
            tx.send(Ok("data: after\n\n".into())).unwrap();
            let next = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                .await
                .unwrap();
            if address.starts_with("127.") {
                assert_eq!(next.unwrap().unwrap(), "data: after\n\n");
            } else {
                assert!(next.is_none());
            }
        }
    }
}

#[tokio::test]
async fn remote_preflights_are_fenced_before_cors() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    for signed_out in [false, true] {
        apply(
            &state,
            if signed_out {
                AccountPresence::SignedOut
            } else {
                AccountPresence::NeverSignedIn
            },
        );
        for address in ["127.0.0.1:9000", "100.64.0.2:9000"] {
            let mut req = request("/api/config", address, "");
            *req.method_mut() = Method::OPTIONS;
            req.headers_mut()
                .insert("origin", "http://localhost:1420".parse().unwrap());
            req.headers_mut()
                .insert("access-control-request-method", "GET".parse().unwrap());
            let router = crate::routes::apply_middleware(
                Router::new().route("/api/config", get(|| async { "ok" })),
                state.clone(),
            );
            let expected = if signed_out && address.starts_with("100.") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::OK
            };
            assert_eq!(router.oneshot(req).await.unwrap().status(), expected);
        }
    }
}

#[tokio::test]
async fn sign_out_cancels_pending_remote_handlers() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let entered = std::sync::Arc::new(tokio::sync::Notify::new());
    let handler_entered = entered.clone();
    let pending = Router::new()
        .route(
            "/api/wait",
            get(move || {
                let entered = handler_entered.clone();
                async move {
                    entered.notify_one();
                    std::future::pending::<String>().await
                }
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            remote::middleware,
        ));
    let response = tokio::spawn(pending.oneshot(request(
        "/api/wait",
        "192.168.1.2:9000",
        "preview-test-token",
    )));
    entered.notified().await;
    apply(&state, AccountPresence::SignedOut);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), response)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn tailnet_bound_signed_out_web_api_requires_instance_token_and_refuses_mesh_traffic() {
    use codeterm_service_core::startup::rest_api::{self, RestApiControl, RestApiPhase};
    use std::sync::Arc;

    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let peer = signed_peer(&state);
    apply(&state, AccountPresence::SignedOut);
    let router = router(&state).layer(axum::middleware::from_fn(
        |mut req: Request, next: axum::middleware::Next| async move {
            req.extensions_mut()
                .insert(codeterm_service_core::service::account_mesh::tailnet::TailnetListener);
            next.run(req).await
        },
    ));
    let control = Arc::new(RestApiControl::new());
    let mut status = control.subscribe_status();
    let server = rest_api::spawn(control.clone(), "127.0.0.1:0".into(), router);
    loop {
        if status.borrow_and_update().phase == RestApiPhase::Listening {
            break;
        }
        status.changed().await.unwrap();
    }
    let address = control.bound_addr().unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (path, token, expected) in [
        ("/api/config", "preview-test-token", StatusCode::OK),
        ("/api/config", "", StatusCode::UNAUTHORIZED),
        ("/api/config", peer.as_str(), StatusCode::UNAUTHORIZED),
        ("/frontend/index.html", "", StatusCode::OK),
    ] {
        assert_eq!(
            client
                .get(format!("http://{address}{path}"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert_eq!(
        client
            .post(format!("http://{address}/api/mesh/relay"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    control.shutdown_and_wait().await.unwrap();
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn tailnet_sign_out_closes_peer_stream_but_preserves_instance_token_stream() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let peer = signed_peer(&state);
    for token in ["preview-test-token", peer.as_str()] {
        apply(
            &state,
            AccountPresence::SignedIn {
                account_id: "account".into(),
            },
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<String, std::io::Error>>();
        let rx = std::sync::Arc::new(parking_lot::Mutex::new(Some(rx)));
        let streaming = Router::new()
            .route(
                "/api/stream",
                get(move || {
                    let rx = rx.lock().take().unwrap();
                    async move {
                        Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async {
                            rx.recv().await.map(|item| (item, rx))
                        }))
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                remote::middleware,
            ));
        let mut req = request("/api/stream", "100.64.0.2:9000", token);
        req.extensions_mut()
            .insert(codeterm_service_core::service::account_mesh::tailnet::TailnetListener);
        let response = streaming.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        tx.send(Ok("before".into())).unwrap();
        assert_eq!(stream.next().await.unwrap().unwrap(), "before");
        apply(&state, AccountPresence::SignedOut);
        if token == peer {
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                    .await
                    .unwrap()
                    .is_none()
            );
        } else {
            tx.send(Ok("after".into())).unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), "after");
        }
    }
}
