//! Registered-router integration across the real relay-to, receiver and loopback transports.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_api::routes::{self, AppState};
use codeterm_service_core::{
    mesh::{
        crown::{self, AuthenticatedOrigin},
        types::{AccessLevel, PairedPeer, PeerAddress, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use tower::ServiceExt;

#[path = "../src/test_support.rs"]
mod test_support;

fn initialize() {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        let data = root.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::env::set_var("CODETERM_CONFIG_DIR", config);
        std::env::set_var("CODETERM_DATA_DIR", data);
        root
    });
    test_support::install_app_version();
}

fn state(fixture: &ServiceFixture, id: &str, port: u16) -> AppState {
    let mut state = test_support::app_state(fixture);
    state.api_port = port;
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;
    let (key, public) = crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.clear();
        mesh.ident.identity.instance_id = id.into();
        mesh.ident.identity.private_key = Some(key);
        mesh.ident.identity.public_key = Some(public);
    }
    state
}

fn install_rpc() {
    use codeterm_service_core::ports::socket::{register, RpcBridge, RpcError};
    fn unavailable() -> RpcError {
        RpcError {
            code: -32000,
            message: "not used by authority integration".into(),
            data: None,
        }
    }
    register(RpcBridge {
        protocol_version: || 1,
        version_skew_gate: |_| None,
        dispatch_service_rpc: Box::new(|method, params, _| {
            Ok(json!({"method":method,"params":params}))
        }),
        dispatch_mesh_rpc: Box::new(|method, params, _, peer| {
            Ok(json!({"method":method,"params":params,"peer":peer}))
        }),
        recover_remote_attempts: |_, _, _, _, _| Ok(()),
        respawn_remote: |_, _, _, _, _, _| Err(unavailable()),
        run_dispatch_startup: |_, _| {},
        start_server: Box::new(|_, _, _, _, _| Err(unavailable().message)),
    });
}

fn signed_agent(state: &AppState, machine: &str, tab: &str) -> String {
    crown::sign_request_with_agent(
        state
            .mesh
            .lock()
            .ident
            .identity
            .private_key
            .as_deref()
            .unwrap(),
        machine,
        AuthenticatedOrigin::Agent,
        Some(&crown::AuthenticatedAgent {
            instance_id: machine.into(),
            tab_id: tab.into(),
            can_spawn: true,
        }),
    )
    .unwrap()
}

async fn relay(app: axum::Router, token: &str, method: &str, path: &str, body: Value) -> Value {
    let (status, response) = request(
        app,
        token,
        "POST",
        "/api/mesh/relay-to",
        json!({
            "target_peer_id":"target", "method":method, "path":path, "body":body,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{path}: {response}");
    assert_eq!(response["status"], 200, "{path}: {response}");
    assert_eq!(response["ok"], true, "{path}: {response}");
    response["data"].clone()
}

fn pair(target: &AppState, source: &AppState) {
    let mesh = source.mesh.lock();
    target.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: mesh.ident.identity.instance_id.clone(),
        instance_name: mesh.ident.identity.instance_id.clone(),
        addresses: vec![PeerAddress::from_ip("127.0.0.1")],
        api_port: source.api_port,
        public_key: mesh.ident.identity.public_key.clone().unwrap(),
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

async fn request(
    app: axum::Router,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("x-codeterm-authenticated-origin", "Owner")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn registered_router_never_mints_mesh_authority_for_a_tab() {
    initialize();
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = state(&fixture, "source", 0);
    let token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = routes::create_router_with_frontend(state, None);
    for method in ["GET", "POST"] {
        let (status, body) = request(
            app.clone(),
            &token,
            method,
            "/api/mesh/debug/sign",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.get("token").is_none(), "{body}");
    }
}

#[tokio::test]
async fn relayed_agent_cannot_change_mesh_administration_state() {
    install_rpc();
    initialize();
    let (mux, tab) = MultiplexerState::new();
    let source_fixture = ServiceFixture::new(mux);
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = state(&source_fixture, "source", 0);
    let target = state(
        &target_fixture,
        "target",
        listener.local_addr().unwrap().port(),
    );
    pair(&source, &target);
    pair(&target, &source);
    let mut victim = target.mesh.lock().paired_peers["source"].clone();
    victim.instance_id = "victim".into();
    victim.instance_name = "Victim".into();
    victim.access_level = AccessLevel::ReadOnly;
    target.mesh.lock().insert_account_peer_for_test(victim);
    let baseline = serde_json::to_value(target.config.lock().clone()).unwrap();
    let config_path = codeterm_paths::config_path().unwrap();
    let persisted_config = std::fs::read(&config_path).ok();
    let original_addresses =
        serde_json::to_value(&target.mesh.lock().paired_peers["victim"].addresses).unwrap();
    let agent = source
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let app = routes::create_router_with_frontend(target.clone(), None);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let app = routes::create_router_with_frontend(source.clone(), None);
    for (path, body) in [
        (
            "/api/mesh/peers/victim/access",
            json!({"access_level":"full"}),
        ),
        ("/api/mesh/enabled", json!({"enabled":false})),
        ("/api/mesh/peers/victim/addr", json!({"ip":"192.0.2.9"})),
        ("/api/mesh/refresh", json!({})),
        ("/api/mesh/announce/all", json!({})),
    ] {
        let (status, response) = request(
            app.clone(),
            &agent,
            "POST",
            "/api/mesh/relay-to",
            json!({
                "target_peer_id":"target", "method":"POST", "path":path, "body":body,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{path}: {response}");
        assert_eq!(response["peer"]["instance_id"], "target", "{response}");
        assert_eq!(response["status"], 403, "{path}: {response}");
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(
            target.mesh.lock().paired_peers["victim"].access_level,
            AccessLevel::ReadOnly
        );
        assert_eq!(
            serde_json::to_value(&target.mesh.lock().paired_peers["victim"].addresses).unwrap(),
            original_addresses
        );
        assert_eq!(
            serde_json::to_value(target.config.lock().clone()).unwrap(),
            baseline
        );
        assert_eq!(std::fs::read(&config_path).ok(), persisted_config);
    }
    // The same receiver and writer admit an owner relay, proving refusal isn't transport failure.
    let (status, response) = request(
        app.clone(),
        "owner-secret",
        "POST",
        "/api/mesh/relay-to",
        json!({
            "target_peer_id":"target", "method":"POST", "path":"/api/mesh/peers/victim/access",
            "body":{"access_level":"full"},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["status"], 200, "{response}");
    assert_eq!(
        target.mesh.lock().paired_peers["victim"].access_level,
        AccessLevel::Full
    );
    // A service credential signed by a paired machine has no administration authority either.
    let service = {
        let mesh = source.mesh.lock();
        crown::sign_request_with_origin(
            mesh.ident.identity.private_key.as_deref().unwrap(),
            "source",
            AuthenticatedOrigin::Service,
        )
        .unwrap()
    };
    let target_app = routes::create_router_with_frontend(target.clone(), None);
    let (status, _) = request(
        target_app,
        &service,
        "POST",
        "/api/mesh/enabled",
        json!({"enabled":false}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::to_value(target.config.lock().clone()).unwrap(),
        baseline
    );
    stop.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn registered_agent_operation_matrix_preserves_owner_local_remote_and_peer_access() {
    initialize();
    install_rpc();
    let (mut mux, local) = MultiplexerState::new();
    mux.get_tab_mut(&local).unwrap().can_spawn = true;
    let workspace = mux.snapshot().workspaces[0].id.clone();
    let recipient = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let (mux, remote_tab) = MultiplexerState::new();
    let remote_fixture = ServiceFixture::new(mux);
    let target = state(&fixture, "target", 0);
    let source = state(&remote_fixture, "source", 0);
    pair(&target, &source);
    let writes =
        std::sync::Arc::new(fixture.bind_recorded_agent_recipient(&recipient, "session", "nonce"));
    let _pty = fixture.bind_test_pty_session(&recipient, "session", "nonce");
    let pty_id = fixture.service.resolve_pty_id(&recipient).unwrap();
    fixture
        .service
        .screen_state
        .process(&pty_id, b"matrix screen");
    let local_token = target
        .svc
        .frontend_pty_env(Some(&local))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let remote_token = signed_agent(&source, "source", &remote_tab);
    let peer = source.mesh.lock().sign_request().unwrap();
    let app = routes::create_router_with_frontend(target.clone(), None);
    for (kind, token) in [
        ("owner", "owner-secret".into()),
        ("local-agent", local_token),
        ("remote-agent", remote_token),
        ("peer", peer),
    ] {
        for path in [
            "/api/identify",
            "/api/config",
            "/api/tabs",
            "/api/mesh/peers",
            "/api/mesh/status",
        ] {
            let (status, body) = request(app.clone(), &token, "GET", path, Value::Null).await;
            assert_eq!(status, StatusCode::OK, "{kind} {path}: {body}");
        }
        let (status, screen) = request(
            app.clone(),
            &token,
            "GET",
            &format!("/api/tabs/{recipient}/text?formatted=false"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {screen}");
        assert!(screen["text"].as_str().unwrap().contains("matrix screen"));
        let (status, saved) = request(app.clone(), &token, "POST", "/api/mem/items", json!({
            "kind":"fact", "title":format!("authority-{kind}"), "body":format!("authority preserved for {kind}"), "skip_extract":true,
        })).await;
        assert_eq!(status, StatusCode::OK, "{kind}: {saved}");
        let id = saved["id"].as_str().unwrap();
        let (status, fetched) = request(
            app.clone(),
            &token,
            "GET",
            &format!("/api/mem/items/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {fetched}");
        assert_eq!(fetched["id"], id);
        if kind == "remote-agent" {
            assert_eq!(
                fetched["author"]["label"],
                format!("agent:source:{remote_tab}")
            );
        }
        let (status, hits) = request(
            app.clone(),
            &token,
            "GET",
            "/api/mem/search?q=authority&supporter=false&rerank=false",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {hits}");
        assert!(
            hits["hits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hit| hit["item"]["id"] == id),
            "{hits}"
        );
        let text = format!("paste-{kind}");
        let (status, paste) = request(
            app.clone(),
            &token,
            "POST",
            "/api/pasteboard",
            json!({"body":text}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {paste}");
        let (status, got) = request(
            app.clone(),
            &token,
            "GET",
            &format!("/api/pasteboard/{}", paste["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {got}");
        assert_eq!(got["body"], text);
        let (status, sent) = request(
            app.clone(),
            &token,
            "POST",
            &format!("/api/tabs/{recipient}/send"),
            json!({"text":format!("matrix-{kind}")}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {sent}");
        assert_eq!(sent["ok"], true);

        if kind == "peer" {
            let (status, rpc) = request(app.clone(), &token, "POST", "/api/mesh/rpc", json!({
                "method":"plan.list", "params":{}, "request_id":"trusted-peer-plan-list", "protocol_version":1,
            })).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(rpc["result"]["method"], "plan.list", "{rpc}");
        }
        // A deliberately unknown provider reaches ordinary validation after actor/delegation admission.
        let (status, spawn) = request(app.clone(), &token, "POST", "/api/agents/spawn", json!({
            "agent":"authority-fixture-no-provider", "title":"Child", "description":"Authority fixture", "role":"Worker",
        })).await;
        assert_ne!(status, StatusCode::FORBIDDEN, "{kind}: {spawn}");
        assert!(
            spawn.to_string().contains("authority-fixture-no-provider"),
            "{kind}: {spawn}"
        );
    }
    assert_eq!(writes.texts.lock().len(), 4);
}

#[tokio::test]
async fn owner_and_agent_mesh_workflows_reach_the_same_registered_target_routes() {
    initialize();
    install_rpc();
    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().can_spawn = true;
    let source_fixture = ServiceFixture::new(mux);
    let (mux, recipient) = MultiplexerState::new();
    let target_fixture = ServiceFixture::new(mux);
    let writes = std::sync::Arc::new(
        target_fixture.bind_recorded_agent_recipient(&recipient, "session", "nonce"),
    );
    let _pty = target_fixture.bind_test_pty_session(&recipient, "session", "nonce");
    let pty_id = target_fixture.service.resolve_pty_id(&recipient).unwrap();
    target_fixture
        .service
        .screen_state
        .process(&pty_id, b"mesh screen");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = state(&source_fixture, "source", 0);
    let target = state(
        &target_fixture,
        "target",
        listener.local_addr().unwrap().port(),
    );
    pair(&source, &target);
    pair(&target, &source);
    let agent = source
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let target_app = routes::create_router_with_frontend(target.clone(), None);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            target_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let peer_client = codeterm_mesh::transport::PeerClient::new_signed(
        "127.0.0.1",
        target.api_port,
        &source.mesh.lock().sign_request().unwrap(),
    );
    let payload = source.mesh.lock().build_sync_payload();
    let sync = peer_client.send_sync(&payload).await.unwrap();
    assert!(sync.peers.iter().any(|peer| peer.instance_id == "target"));
    let inventory = peer_client
        .auth_get_json("/api/mesh/inventory")
        .await
        .unwrap();
    assert_eq!(inventory["machine_id"], "target");
    let app = routes::create_router_with_frontend(source, None);
    for (kind, token) in [("owner", "owner-secret"), ("agent", agent.as_str())] {
        for path in [
            "/api/identify",
            "/api/config",
            "/api/tabs",
            "/api/mesh/peers",
            "/api/mesh/status",
        ] {
            relay(app.clone(), token, "GET", path, Value::Null).await;
        }
        let screen = relay(
            app.clone(),
            token,
            "GET",
            &format!("/api/tabs/{recipient}/text?formatted=false"),
            Value::Null,
        )
        .await;
        assert!(screen["text"].as_str().unwrap().contains("mesh screen"));
        let sent = relay(
            app.clone(),
            token,
            "POST",
            &format!("/api/tabs/{recipient}/send"),
            json!({"text":format!("workflow-{kind}")}),
        )
        .await;
        assert_eq!(sent["ok"], true);

        let saved = relay(app.clone(), token, "POST", "/api/mem/items", json!({
            "kind":"fact", "title":format!("mesh-{kind}"), "body":format!("mesh authority {kind}"), "skip_extract":true,
        })).await;
        let id = saved["id"].as_str().unwrap();
        let got = relay(
            app.clone(),
            token,
            "GET",
            &format!("/api/mem/items/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(got["id"], id);
        relay(
            app.clone(),
            token,
            "GET",
            "/api/mem/search?q=mesh&supporter=false&rerank=false",
            Value::Null,
        )
        .await;
        let paste = relay(
            app.clone(),
            token,
            "POST",
            "/api/pasteboard",
            json!({"body":format!("mesh-{kind}")}),
        )
        .await;
        let got = relay(
            app.clone(),
            token,
            "GET",
            &format!("/api/pasteboard/{}", paste["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
        assert_eq!(got["body"], format!("mesh-{kind}"));
        for method in ["plan.task.remote_ack", "plan.task.remote_report"] {
            let result = relay(app.clone(), token, "POST", "/api/mesh/rpc", json!({
                "method":method, "protocol_version":1, "request_id":format!("matrix-{kind}-{method}"),
                "params":{"worker_tab_id":tab,"report":{"reporter_tab_id":tab}},
            })).await;
            assert!(result.get("error").is_none(), "{kind} {method}: {result}");
            assert_eq!(result["result"]["peer"], "source", "{result}");
            assert_eq!(result["result"]["method"], method);
        }
    }
    assert_eq!(writes.texts.lock().len(), 2);
    stop.send(()).unwrap();
    server.await.unwrap();
}
