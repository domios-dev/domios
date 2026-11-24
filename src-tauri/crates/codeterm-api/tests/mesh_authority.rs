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
        domios_version: None,
        domios_build: None,
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
    relay(app.clone(), &agent, "GET", "/api/mesh/status", Value::Null).await;
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
    let access_path = "/api/mesh/peers/victim/access";
    let access_body = json!({"access_level":"full"});
    let (status, response) = request(
        app,
        "owner-secret",
        "POST",
        "/api/mesh/relay-to",
        json!({
            "target_peer_id":"target", "method":"POST", "path":access_path, "body":access_body,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["status"], 403, "{response}");
    assert_eq!(
        response["data"]["error"]["code"],
        codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED
    );
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(
        target.mesh.lock().paired_peers["victim"].access_level,
        AccessLevel::ReadOnly
    );
    assert_eq!(std::fs::read(&config_path).ok(), persisted_config);
    let target_app = routes::create_router_with_frontend(target.clone(), None);
    let (status, response) = request(
        target_app.clone(),
        "owner-secret",
        "POST",
        access_path,
        access_body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["ok"], true, "{response}");
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
async fn registered_tab_close_routes_refuse_unscoped_peer_access() {
    initialize();
    let (mux, source_tab) = MultiplexerState::new();
    let source_fixture = ServiceFixture::new(mux);
    let source = state(&source_fixture, "source", 0);
    let (mut mux, initial) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&initial).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let target_fixture = ServiceFixture::new(mux);
    let target = state(&target_fixture, "target", 0);
    pair(&target, &source);
    let app = routes::create_router_with_frontend(target.clone(), None);
    let paths = [
        format!("/api/workspaces/{workspace}/tabs/{tab}/close"),
        format!("/api/tabs/{tab}/close"),
    ];
    for access in [
        AccessLevel::Full,
        AccessLevel::Custom(vec![codeterm_mesh::types::Permission::Admin]),
    ] {
        assert!(target.mesh.lock().set_peer_access("source", access));
        for path in &paths {
            for agent in [false, true] {
                let token = || {
                    if agent {
                        return signed_agent(&source, "source", &source_tab);
                    }
                    crown::sign_request_with_origin(
                        source
                            .mesh
                            .lock()
                            .ident
                            .identity
                            .private_key
                            .as_deref()
                            .unwrap(),
                        "source",
                        AuthenticatedOrigin::Owner,
                    )
                    .unwrap()
                };
                let (direct, _) = request(app.clone(), &token(), "POST", path, Value::Null).await;
                assert_eq!(direct, StatusCode::FORBIDDEN, "{path} agent={agent}");
                let (relay, data) = request(
                    app.clone(),
                    &token(),
                    "POST",
                    "/api/mesh/relay",
                    json!({"method":"POST","path":path,"body":null}),
                )
                .await;
                assert_eq!(relay, StatusCode::OK, "{path} agent={agent}: {data}");
                assert_eq!(data["status"], 403, "{path} agent={agent}: {data}");
                assert!(target.svc.mux.read().get_tab(&tab).is_some());
            }
        }
    }
    let local_agent = target
        .svc
        .frontend_pty_env(Some(&initial))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let (agent, _) = request(app.clone(), &local_agent, "POST", &paths[0], Value::Null).await;
    assert_eq!(agent, StatusCode::FORBIDDEN);
    assert!(target.svc.mux.read().get_tab(&tab).is_some());

    let (mismatch, _) = request(
        app.clone(),
        "owner-secret",
        "POST",
        &format!("/api/workspaces/wrong-workspace/tabs/{tab}/close"),
        Value::Null,
    )
    .await;
    assert_eq!(mismatch, StatusCode::NOT_FOUND);
    assert!(target.svc.mux.read().get_tab(&tab).is_some());
    let (local, result) = request(app, "owner-secret", "POST", &paths[0], Value::Null).await;
    assert_eq!(local, StatusCode::OK, "{result}");
    assert!(target.svc.mux.read().get_tab(&tab).is_none());
}

#[tokio::test]
async fn mesh_team_rpc_and_surface_close_require_a_local_caller() {
    initialize();
    install_rpc();
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = state(&fixture, "source", 0);
    let target = state(&fixture, "target", 0);
    pair(&target, &source);
    let app = routes::create_router_with_frontend(target.clone(), None);
    let signed = |origin| {
        crown::sign_request_with_origin(
            source
                .mesh
                .lock()
                .ident
                .identity
                .private_key
                .as_deref()
                .unwrap(),
            "source",
            origin,
        )
        .unwrap()
    };
    for access in [
        AccessLevel::Custom(vec![codeterm_mesh::types::Permission::Admin]),
        AccessLevel::Full,
    ] {
        assert!(target.mesh.lock().set_peer_access("source", access));
        for origin in [AuthenticatedOrigin::Owner, AuthenticatedOrigin::Mesh] {
            let rpc = json!({
                "method":"team.close", "request_id":"team-close-denied", "protocol_version":1,
                "params":{"team_id":"team-fixture","confirm":"team-fixture","force":true}
            });
            let (status, direct) = request(
                app.clone(),
                &signed(origin),
                "POST",
                "/api/mesh/rpc",
                rpc.clone(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(direct["error"]["code"], -32003, "{direct}");
            let (status, relayed) = request(
                app.clone(),
                &signed(origin),
                "POST",
                "/api/mesh/relay",
                json!({"method":"POST","path":"/api/mesh/rpc","body":rpc}),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(relayed["data"]["error"]["code"], -32003, "{relayed}");

            let surface = json!({"tab_id":"surface-fixture"});
            let (status, _) = request(
                app.clone(),
                &signed(origin),
                "POST",
                "/api/surfaces/close",
                surface.clone(),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            let (status, relayed) = request(
                app.clone(),
                &signed(origin),
                "POST",
                "/api/mesh/relay",
                json!({"method":"POST","path":"/api/surfaces/close","body":surface}),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(relayed["status"], 403, "{relayed}");
        }
    }
}

#[tokio::test]
async fn workspace_rest_close_refuses_signed_peer_owner() {
    initialize();
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("Disposable".into(), None).id;
    let tab = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let source = state(&fixture, "source", 0);
    let target = state(&fixture, "target", 0);
    pair(&target, &source);
    assert!(target.mesh.lock().set_peer_access(
        "source",
        AccessLevel::Custom(vec![codeterm_mesh::types::Permission::ManageWorkspaces,])
    ));
    let token = crown::sign_request_with_origin(
        source
            .mesh
            .lock()
            .ident
            .identity
            .private_key
            .as_deref()
            .unwrap(),
        "source",
        AuthenticatedOrigin::Owner,
    )
    .unwrap();
    let (status, _) = request(
        routes::create_router_with_frontend(target.clone(), None),
        &token,
        "POST",
        &format!("/api/workspaces/{workspace}/close"),
        json!({"force":true,"confirm":workspace}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(target.svc.mux.read().get_tab(&tab).is_some());
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
            "agent":"authority-fixture-no-provider", "title":"Child", "description":"Authority fixture", "role":"worker",
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
    let app = routes::create_router_with_frontend(source.clone(), None);
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
            if kind == "agent" {
                let refused = relay(app.clone(), token, "POST", "/api/mesh/rpc", json!({
                    "method":method, "protocol_version":1,
                    "request_id":format!("matrix-forged-{method}"),
                    "params":{"worker_tab_id":"foreign-worker","report":{"reporter_tab_id":"foreign-worker"}},
                })).await;
                assert_eq!(refused["error"]["code"], -32003, "{refused}");
                assert_eq!(
                    refused["error"]["message"],
                    "Remote lifecycle request must match the signed originating agent"
                );
            }
        }
    }
    assert_eq!(writes.texts.lock().len(), 2);
    let direct_owner = {
        let mesh = source.mesh.lock();
        crown::sign_request_with_origin(
            mesh.ident.identity.private_key.as_deref().unwrap(),
            "source",
            AuthenticatedOrigin::Owner,
        )
        .unwrap()
    };
    let direct_agent = signed_agent(&source, "source", &tab);
    for (kind, token, direct) in [
        ("owner", "owner-secret", false),
        ("agent", agent.as_str(), false),
        ("owner", direct_owner.as_str(), true),
        ("agent", direct_agent.as_str(), true),
    ] {
        for (owner_peer, owner_agent, allowed) in [
            ("source", tab.as_str(), true),
            ("source", "foreign-agent", kind == "owner"),
            ("foreign-peer", tab.as_str(), false),
        ] {
            let target_tab = {
                let mut mux = target.svc.mux.write();
                let workspace = mux.find_workspace_for_tab(&recipient).unwrap();
                let created = mux.create_tab(&workspace).unwrap();
                mux.get_tab_mut(&created).unwrap().ownership =
                    Some(codeterm_types::pty::RemoteOwnership {
                        peer_id: owner_peer.into(),
                        peer_name: owner_peer.into(),
                        controller_tab_id: Some(owner_agent.into()),
                        controller_title: owner_agent.into(),
                        team: None,
                    });
                created
            };
            let path = format!("/api/tabs/{target_tab}/close?force=true");
            let (status, answer) = if direct {
                let (status, answer) = request(
                    routes::create_router_with_frontend(target.clone(), None),
                    token,
                    "POST",
                    &path,
                    Value::Null,
                )
                .await;
                (
                    StatusCode::OK,
                    json!({"status":status.as_u16(), "data":answer}),
                )
            } else {
                request(
                    app.clone(),
                    token,
                    "POST",
                    "/api/mesh/relay-to",
                    json!({
                        "target_peer_id":"target", "method":"POST", "path":path, "body":null,
                    }),
                )
                .await
            };
            assert_eq!(status, StatusCode::OK, "{answer}");
            assert_eq!(
                answer["status"],
                if allowed { 200 } else { 403 },
                "{kind}: {answer}"
            );
            assert_eq!(
                target.svc.mux.read().get_tab(&target_tab).is_none(),
                allowed
            );
        }
    }

    let shared_tab = {
        let mut mux = target.svc.mux.write();
        let workspace = mux.find_workspace_for_tab(&recipient).unwrap();
        mux.create_tab(&workspace).unwrap()
    };
    for (token, access, expected) in [
        (agent.as_str(), AccessLevel::Full, 403),
        ("owner-secret", AccessLevel::ReadOnly, 403),
        (
            "owner-secret",
            AccessLevel::Custom(vec![codeterm_mesh::types::Permission::Admin]),
            403,
        ),
        ("owner-secret", AccessLevel::Full, 200),
    ] {
        assert!(target.mesh.lock().set_peer_access("source", access));
        let (status, answer) = request(
            app.clone(),
            token,
            "POST",
            "/api/mesh/relay-to",
            json!({
                "target_instance_id":"target", "method":"POST",
                "path":format!("/api/tabs/{shared_tab}/close"),
                "body":{"as_owner":true}
            }),
        )
        .await;
        let received = if status == StatusCode::OK {
            answer["status"].as_u64().unwrap()
        } else {
            u64::from(status.as_u16())
        };
        assert_eq!(received, expected, "{status}: {answer}");
        assert_eq!(
            target.svc.mux.read().get_tab(&shared_tab).is_none(),
            expected == 200
        );
    }
    stop.send(()).unwrap();
    server.await.unwrap();
}
