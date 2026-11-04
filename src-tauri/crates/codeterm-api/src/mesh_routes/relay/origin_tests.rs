use super::*;
use crate::auth::{authenticated_origin, AuthenticatedOrigin};
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::post,
    Router,
};
use codeterm_service_core::{
    mesh::{
        transport::PeerClient,
        types::{AccessLevel, PairedPeer, RelayCommand, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::{
        state::MultiplexerState,
        types::{SurfaceBacking, TabBacking},
    },
    service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn state(fixture: &ServiceFixture, id: &str) -> AppState {
    let state = crate::test_support::app_state(fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;
    let (key, public) = codeterm_mesh::crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.clear();
        mesh.ident.identity.instance_id = id.into();
        mesh.ident.identity.private_key = Some(key);
        mesh.ident.identity.public_key = Some(public);
    }
    state
}

fn pair(target: &AppState, source: &AppState) {
    let source = source.mesh.lock();
    target.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: source.ident.identity.instance_id.clone(),
        instance_name: source.ident.identity.instance_id.clone(),
        addresses: vec![],
        api_port: 0,
        public_key: source.ident.identity.public_key.clone().unwrap(),
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
}

fn headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers.insert("x-codeterm-authenticated-origin", "Owner".parse().unwrap());
    headers
}

fn wire_request(request: reqwest::Request) -> Request<Body> {
    let body = request
        .body()
        .and_then(|body| body.as_bytes())
        .unwrap_or_default()
        .to_vec();
    let mut wire = Request::builder()
        .method(request.method().clone())
        .uri(request.url().path())
        .body(Body::from(body))
        .unwrap();
    *wire.headers_mut() = request.headers().clone();
    wire
}

fn surface(fixture: &ServiceFixture) -> String {
    use codeterm_plugin::surfaces::{SurfaceDecl, SurfaceRenderer, SurfaceRestore};
    fixture
        .service
        .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
        .register(
            "origin-test",
            std::sync::Arc::new(vec![SurfaceDecl {
                id: "browser".into(),
                title: "Origin".into(),
                renderer: SurfaceRenderer::Stream,
                agent_bound: false,
                restore: SurfaceRestore::Placeholder,
                runtime: Some("browser".into()),
                chrome: None,
                icon: None,
                agent_verbs: vec![],
                state_invalidation: false,
            }]),
        );
    let mut mux = fixture.service.mux.write();
    let workspace = mux.snapshot().workspaces[0].id.clone();
    let tab = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&tab).unwrap().backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: "origin-test".into(),
        surface_id: "browser".into(),
        instance_id: "origin".into(),
        owner_label: false,
        resume_url: None,
        resume_profile: None,
    });
    tab
}

#[tokio::test]
async fn relay_wire_round_trips_preserve_agent_and_owner_authority_through_nested_hops() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let first = state(&fixture, "first");
    let second = state(&fixture, "second");
    pair(&first, &second);
    pair(&second, &first);
    let surface = surface(&fixture);
    let agent = first
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let peer = second.mesh.lock().sign_request().unwrap();
    let app = Router::new()
        .merge(crate::transport_capabilities::router())
        .route(
            "/api/surfaces/{tab_id}/call",
            post(crate::surface_routes::call),
        )
        .layer(axum::middleware::from_fn_with_state(
            first.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(first.clone());
    for (label, token, expected) in [
        ("agent", agent, AuthenticatedOrigin::Agent),
        ("owner", "owner-secret".into(), AuthenticatedOrigin::Owner),
        ("peer", peer, AuthenticatedOrigin::Mesh),
    ] {
        let mut caller_headers = headers(&token);
        assert_eq!(
            authenticated_origin(&first, &caller_headers).unwrap(),
            expected
        );
        for hops in 0..3 {
            for (method, path, body) in [
                ("GET", "/api/system/default-cwd".into(), Value::Null),
                (
                    "POST",
                    format!("/api/surfaces/{surface}/call"),
                    json!({
                        "method":"profiles.create", "args":{"name":format!("{label}-{hops}")},
                    }),
                ),
            ] {
                let cmd = RelayCommand {
                    method: method.into(),
                    path,
                    body,
                    delivery_id: None,
                };
                let origin = authenticated_origin(&first, &caller_headers).unwrap();
                let request = origin::loopback_request(
                    &first,
                    origin,
                    &cmd,
                    &cmd.body,
                    crate::auth::authenticated_agent(&first, &caller_headers)
                        .unwrap()
                        .as_ref(),
                )
                .unwrap();
                let response = app.clone().oneshot(wire_request(request)).await.unwrap();
                if method == "GET" {
                    assert_eq!(
                        response.status(),
                        if expected == AuthenticatedOrigin::Agent {
                            StatusCode::FORBIDDEN
                        } else {
                            StatusCode::OK
                        },
                        "{label} hops={hops}"
                    );
                } else {
                    assert_eq!(response.status(), StatusCode::OK);
                    let body: Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 65536).await.unwrap(),
                    )
                    .unwrap();
                    if expected == AuthenticatedOrigin::Agent {
                        assert!(
                            body["error"].as_str().unwrap().starts_with("user_only:"),
                            "{body}"
                        );
                    } else {
                        assert!(body.get("result").is_some(), "{label} hops={hops}: {body}");
                    }
                }
            }
            let cmd = RelayCommand {
                method: "POST".into(),
                path: "/api/mesh/relay-to".into(),
                body: json!({
                    "target_peer_id":"first", "method":"GET", "path":"/api/system/default-cwd",
                }),
                delivery_id: None,
            };
            let client = origin::peer_client(
                &first,
                &caller_headers,
                PeerClient::new_signed("127.0.0.1", 1, "discarded"),
            )
            .unwrap();
            let outward = client.relay_request(&cmd).unwrap();
            assert_eq!(
                authenticated_origin(&second, outward.headers()).unwrap(),
                expected
            );
            let received_origin = authenticated_origin(&second, outward.headers()).unwrap();
            let loopback = origin::loopback_request(
                &second,
                received_origin,
                &cmd,
                &cmd.body,
                crate::auth::authenticated_agent(&second, outward.headers())
                    .unwrap()
                    .as_ref(),
            )
            .unwrap();
            let client = origin::peer_client(
                &second,
                loopback.headers(),
                PeerClient::new_signed("127.0.0.1", 1, "discarded"),
            )
            .unwrap();
            let returning = client.relay_request(&cmd).unwrap();
            assert_eq!(
                authenticated_origin(&first, returning.headers()).unwrap(),
                expected
            );
            caller_headers = returning.headers().clone();
            caller_headers.insert("x-codeterm-authenticated-origin", "Owner".parse().unwrap());
        }
    }
}

#[tokio::test]
async fn relay_to_v2_peer_returns_typed_actionable_upgrade_before_sending() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let first = state(&fixture, "first");
    let second = state(&fixture, "second");
    pair(&first, &second);
    first
        .mesh
        .lock()
        .paired_peers
        .get_mut("second")
        .unwrap()
        .relay_protocol_version = 2;
    let app = Router::new()
        .route("/api/mesh/relay-to", post(mesh_relay_to_peer))
        .layer(axum::middleware::from_fn_with_state(
            first.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(first);
    let request = Request::builder().method("POST").uri("/api/mesh/relay-to")
        .header("authorization", "Bearer owner-secret").header("content-type", "application/json")
        .body(Body::from(json!({"target_peer_id":"second", "method":"GET", "path":"/api/system/default-cwd", "body":null}).to_string())).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(
        body["code"],
        codeterm_mesh::contract::delivery::PEER_RELAY_PROTOCOL_MISMATCH
    );
    assert_eq!(body["relay_protocol_version"], 2);
    assert_eq!(body["required_relay_protocol_version"], 3);
    assert_eq!(body["bytes_written"], false);
    assert!(body["message"].as_str().unwrap().contains("update it"));
}

#[tokio::test]
async fn todo_review_never_treats_a_relay_without_local_actor_as_the_owner() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    let state = state(&fixture, "todo-origin");
    let written = state
        .svc
        .tab_todo_write(
            &tab,
            &tab,
            vec![
                serde_json::from_value(json!({"content":"Preserve authority", "state":"open"}))
                    .unwrap(),
            ],
        )
        .await
        .unwrap()
        .checklist;
    let app = Router::new()
        .merge(crate::tab_todo_routes::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state.clone());
    let cmd = RelayCommand {
        method: "POST".into(),
        path: format!("/api/tabs/{tab}/todos/review"),
        body: json!({
            "generation":written.generation, "revision":written.revision,
        }),
        delivery_id: None,
    };
    for origin in [
        AuthenticatedOrigin::Agent,
        AuthenticatedOrigin::Mesh,
        AuthenticatedOrigin::Service,
    ] {
        let request = origin::loopback_request(&state, origin, &cmd, &cmd.body, None).unwrap();
        let response = app.clone().oneshot(wire_request(request)).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin:?}");
        assert_eq!(
            state.svc.tab_todo_list(&tab, None).await.unwrap().unwrap(),
            written
        );
    }
    let request =
        origin::loopback_request(&state, AuthenticatedOrigin::Owner, &cmd, &cmd.body, None)
            .unwrap();
    let response = app.oneshot(wire_request(request)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let persisted = state.svc.tab_todo_list(&tab, None).await.unwrap().unwrap();
    assert!(persisted.proposal.is_some());
    assert!(persisted.revision > written.revision);
}

#[tokio::test]
async fn agent_relay_cannot_spend_peer_workspace_or_rpc_authority() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let first = state(&fixture, "first");
    let second = state(&fixture, "second");
    pair(&first, &second);
    let workspace = first.svc.mux.read().snapshot().workspaces[0].id.clone();
    let cmd = RelayCommand {
        method: "POST".into(),
        path: format!("/api/workspaces/{workspace}/close"),
        body: json!({"force":true}),
        delivery_id: None,
    };
    let refusal = perform_relay(&first, "second", &cmd, AuthenticatedOrigin::Agent, None)
        .await
        .unwrap_err();
    assert_eq!(refusal.0, StatusCode::FORBIDDEN);
    assert!(first.svc.mux.read().get_workspace(&workspace).is_some());
    let app = Router::new()
        .route("/api/mesh/rpc", post(crate::mesh_routes::rpc::mesh_rpc))
        .layer(axum::middleware::from_fn_with_state(
            first.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(first.clone());
    let cmd = RelayCommand {
        method: "POST".into(),
        path: "/api/mesh/rpc".into(),
        body: json!({
            "request_id":"agent-rpc", "method":"plan.list", "params":{},
        }),
        delivery_id: None,
    };
    let request =
        origin::loopback_request(&first, AuthenticatedOrigin::Agent, &cmd, &cmd.body, None)
            .unwrap();
    let response = app.oneshot(wire_request(request)).await.unwrap();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["error"]["code"], -32003);
}

#[tokio::test]
async fn agent_mesh_send_and_tab_list_succeed_without_owner_authority() {
    codeterm_service_core::ports::socket::register_test_stub();
    let (mux, source_tab) = MultiplexerState::new();
    let source_fixture = ServiceFixture::new(mux);
    let first = state(&source_fixture, "origin-machine");
    let (mux, target_tab) = MultiplexerState::new();
    let target_fixture = ServiceFixture::new(mux);
    let second = state(&target_fixture, "target-machine");
    pair(&first, &second);
    pair(&second, &first);
    let writes = target_fixture.bind_recorded_agent_recipient(
        &target_tab,
        "recorded-session",
        "recorded-nonce",
    );
    let token = first
        .svc
        .frontend_pty_env(Some(&source_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = Router::new()
        .merge(crate::transport_capabilities::router())
        .route(
            "/api/tabs",
            axum::routing::get(crate::tab_routes::list_tabs),
        )
        .route("/api/tabs/{id}/send", post(crate::tab_routes::send_text))
        .route(
            "/api/tabs/{id}/chat-send",
            post(crate::tab_routes::chat_send_text),
        )
        .layer(axum::middleware::from_fn_with_state(
            second.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(second.clone());
    let mut caller = headers(&token);
    for (method, path, body, status) in [
        ("GET", "/api/tabs".into(), Value::Null, StatusCode::OK),
        (
            "GET",
            "/api/system/default-cwd".into(),
            Value::Null,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            format!("/api/tabs/{target_tab}/send"),
            json!({"text":"remote-agent-payload"}),
            StatusCode::OK,
        ),
        (
            "POST",
            format!("/api/tabs/{target_tab}/chat-send"),
            json!({"text":"remote-chat-refused"}),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let cmd = RelayCommand {
            method: method.into(),
            path,
            body,
            delivery_id: None,
        };
        let client = origin::peer_client(
            &first,
            &caller,
            PeerClient::new_signed("127.0.0.1", 1, "discarded"),
        )
        .unwrap();
        let outward = client.relay_request(&cmd).unwrap();
        let agent = crate::auth::authenticated_agent(&second, outward.headers())
            .unwrap()
            .unwrap();
        assert_eq!(agent.tab_id, source_tab);
        assert_eq!(agent.instance_id, "origin-machine");
        let request = origin::loopback_request(
            &second,
            AuthenticatedOrigin::Agent,
            &cmd,
            &cmd.body,
            Some(&agent),
        )
        .unwrap();
        let response = app.clone().oneshot(wire_request(request)).await.unwrap();
        assert_eq!(response.status(), status);
        let body = to_bytes(response.into_body(), 65536).await.unwrap();
        if method == "POST" && status == StatusCode::OK {
            let result: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(result["ok"], true);
        } else if status == StatusCode::OK {
            let tabs: Value = serde_json::from_slice(&body).unwrap();
            assert!(tabs
                .as_array()
                .unwrap()
                .iter()
                .any(|tab| tab["id"] == target_tab));
        }
        caller.insert("x-codeterm-authenticated-origin", "Owner".parse().unwrap());
    }
    assert_eq!(writes.texts.lock().len(), 1);
    assert!(writes.texts.lock()[0].contains("remote-agent-payload"));
}
