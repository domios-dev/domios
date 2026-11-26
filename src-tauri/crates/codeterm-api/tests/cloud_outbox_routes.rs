//! Registered-router proof that every `OutboundOperation` outer and inner route
//! matches a real `codeterm-api` router entry and respects origin-binding rules.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_api::{
    mesh_routes::relay::{
        MAX_PER_PEER_NOTIFICATIONS, PEER_NOTIFICATION_RATE_LIMITED, REMOTE_NOTIFICATION_REJECTED,
    },
    routes::{self, AppState},
};
use codeterm_mesh::cloud_outbox::{
    CallerContext, OutboundOperation, OutboundPeerRequest, WORKER_ACK_RPC_METHOD,
    WORKER_REPORT_RPC_METHOD,
};
use codeterm_service_core::{
    mesh::{
        crown::{self, AuthenticatedAgent, AuthenticatedOrigin},
        types::{
            AccessLevel, PairedPeer, PeerAddress, PeerAnnounce, Permission, RELAY_PROTOCOL_VERSION,
        },
        version::VERSION_HEADER,
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

fn install_rpc() {
    use codeterm_service_core::ports::socket::{register, RpcBridge, RpcError};
    fn unavailable() -> RpcError {
        RpcError {
            code: -32000,
            message: "not used by cloud outbox route test".into(),
            data: None,
        }
    }
    register(RpcBridge {
        protocol_version: || 1,
        version_skew_gate: |_| None,
        dispatch_service_rpc: Box::new(|method, params, _| {
            Ok(json!({ "method": method, "params": params }))
        }),
        dispatch_mesh_rpc: Box::new(|method, params, _, peer| {
            Ok(json!({ "method": method, "params": params, "peer": peer }))
        }),
        recover_remote_attempts: |_, _, _, _, _| Ok(()),
        respawn_remote: |_, _, _, _, _, _| Err(unavailable()),
        run_dispatch_startup: |_, _| {},
        start_server: Box::new(|_, _, _, _, _| Err(unavailable().message)),
    });
}

fn peer_state(fixture: &ServiceFixture, id: &str, port: u16) -> AppState {
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

fn pair(target: &AppState, source: &AppState) {
    pair_with_access(target, source, None, AccessLevel::Full);
}

fn pair_with_access(
    target: &AppState,
    source: &AppState,
    instance_name: Option<&str>,
    access_level: AccessLevel,
) {
    let mesh = source.mesh.lock();
    let instance_id = mesh.ident.identity.instance_id.clone();
    target.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: instance_id.clone(),
        instance_name: instance_name.map(str::to_owned).unwrap_or(instance_id),
        addresses: vec![PeerAddress::from_ip("127.0.0.1")],
        api_port: source.api_port,
        public_key: mesh.ident.identity.public_key.clone().unwrap(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
}

fn sign_for_caller(source: &AppState, caller: &CallerContext) -> String {
    let mesh = source.mesh.lock();
    let key = mesh.ident.identity.private_key.as_deref().unwrap();
    let instance_id = mesh.ident.identity.instance_id.as_str();
    crown::sign_request_with_agent(key, instance_id, caller.origin, caller.agent.as_ref()).unwrap()
}

async fn send_signed(
    app: axum::Router,
    token: &str,
    method: &str,
    path: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header(VERSION_HEADER, test_support::TEST_APP_VERSION)
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

fn build_valid_operation_request(op: OutboundOperation, source_id: &str) -> OutboundPeerRequest {
    let agent_ctx = CallerContext {
        origin: AuthenticatedOrigin::Agent,
        agent: Some(AuthenticatedAgent {
            instance_id: source_id.into(),
            tab_id: "worker-tab-1".into(),
            can_spawn: false,
        }),
        expires_at_ms: None,
    };
    let (caller_context, body_value) = match op {
        OutboundOperation::Announce => (
            CallerContext {
                origin: AuthenticatedOrigin::Mesh,
                agent: None,
                expires_at_ms: None,
            },
            serde_json::to_value(PeerAnnounce {
                inbound_only: false,
                machine_identity: None,
                domios_version: None,
                domios_build: None,
                instance_id: source_id.into(),
                instance_name: source_id.into(),
                addresses: vec![PeerAddress::from_ip("127.0.0.1")],
                api_port: 7686,
                online: true,
                web_ui: None,
                relay_protocol_version: RELAY_PROTOCOL_VERSION,
            })
            .unwrap(),
        ),
        OutboundOperation::WorkerAck => (
            agent_ctx,
            json!({
                "method": WORKER_ACK_RPC_METHOD,
                "request_id": "route-test-ack-1",
                "protocol_version": 1,
                "params": {
                    "plan_id": "pl_1",
                    "task_id": "task_1",
                    "attempt_id": "att_1",
                    "worker_tab_id": "worker-tab-1",
                    "session_binding_nonce": "nonce-1"
                }
            }),
        ),
        OutboundOperation::WorkerReport => (
            agent_ctx,
            json!({
                "method": WORKER_REPORT_RPC_METHOD,
                "request_id": "route-test-report-1",
                "protocol_version": 1,
                "params": {
                    "plan_id": "pl_1",
                    "task_id": "task_1",
                    "attempt_id": "att_1",
                    "worker_tab_id": "worker-tab-1",
                    "session_binding_nonce": "nonce-1",
                    "report": {
                        "id": "remote-report:att_1",
                        "message": "completed",
                        "status": "in_review",
                        "reporter_tab_id": "worker-tab-1"
                    }
                }
            }),
        ),
        OutboundOperation::Notification => (
            CallerContext {
                origin: AuthenticatedOrigin::Mesh,
                agent: None,
                expires_at_ms: None,
            },
            json!({
                "title": "Cloud worker status",
                "body": "all green",
                "severity": "info",
                "level": "verbose"
            }),
        ),
        OutboundOperation::PlanRpc => (
            CallerContext {
                origin: AuthenticatedOrigin::Owner,
                agent: None,
                expires_at_ms: None,
            },
            json!({
                "method": "plan.get",
                "request_id": "route-test-plan-1",
                "protocol_version": 1,
                "params": { "plan_id": "pl_1" }
            }),
        ),
    };

    OutboundPeerRequest {
        account_id: "acct-1".into(),
        source_installation_id: source_id.into(),
        target_installation_id: "target".into(),
        operation: op,
        caller_context,
        body: serde_json::to_vec(&body_value).unwrap(),
    }
}

#[tokio::test]
async fn every_outbound_operation_maps_to_registered_outer_and_inner_router_entries() {
    install_rpc();
    initialize();

    let source_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_port = listener.local_addr().unwrap().port();

    let source = peer_state(&source_fixture, "source", 0);
    let target = peer_state(&target_fixture, "target", target_port);
    pair(&target, &source);

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let target_app = routes::create_router_with_frontend(target.clone(), None);
    let serve_app = target_app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            serve_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });

    for op in OutboundOperation::ALL {
        let req = build_valid_operation_request(op, "source");
        let wire = req
            .to_outer_wire_request()
            .expect("valid outer wire request");
        let token = sign_for_caller(&source, &req.caller_context);

        let (status, response) = send_signed(
            target_app.clone(),
            &token,
            wire.route.method,
            wire.route.path,
            &wire.body,
        )
        .await;

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "{op:?} outer route {} {} is not registered",
            wire.route.method,
            wire.route.path
        );
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{op:?} outer method {} {} not allowed",
            wire.route.method,
            wire.route.path
        );
        assert_eq!(status, StatusCode::OK, "{op:?} failed: {response}");

        match op {
            OutboundOperation::Announce => {
                assert_eq!(response["ok"], true, "{response}");
            }
            OutboundOperation::WorkerAck => {
                assert_eq!(response["ok"], true, "{response}");
                assert_eq!(response["data"]["result"]["method"], WORKER_ACK_RPC_METHOD);
                assert_eq!(response["data"]["result"]["peer"], "source");
            }
            OutboundOperation::WorkerReport => {
                assert_eq!(response["ok"], true, "{response}");
                assert_eq!(
                    response["data"]["result"]["method"],
                    WORKER_REPORT_RPC_METHOD
                );
                assert_eq!(response["data"]["result"]["peer"], "source");
            }
            OutboundOperation::Notification => {
                assert_eq!(response["ok"], true, "{response}");
                assert_eq!(response["status"], 200, "{response}");
                let notif = target.svc.notif.lock();
                let notifs = notif.list(false);
                assert!(
                    notifs.iter().any(|n| n.title == "Cloud worker status"
                        && n.peer_name.as_deref() == Some("source")
                        && n.tab_id.is_none()
                        && n.workspace_id.is_none()
                        && n.target_tab_id.is_none()
                        && n.chat_message_id.is_none()
                        && n.actions.is_none()
                        && n.surface_offer.is_none()
                        && n.answer.is_none()),
                    "expected display-only relayed notification tagged with peer_name=source and no local target or actions"
                );
            }
            OutboundOperation::PlanRpc => {
                assert_eq!(response["ok"], true, "{response}");
                assert_eq!(response["data"]["result"]["method"], "plan.get");
            }
        }

        // Also verify direct outer route when registered (e.g. POST /api/mesh/rpc).
        if let Some(direct) = op.direct_outer_route() {
            if direct != wire.route {
                let direct_token = sign_for_caller(&source, &req.caller_context);
                let mut direct_body: Value = serde_json::from_slice(&req.body).unwrap();
                if let Some(id) = direct_body.get("request_id").and_then(Value::as_str) {
                    direct_body["request_id"] = json!(format!("{id}-direct"));
                }
                let (direct_status, direct_resp) = send_signed(
                    target_app.clone(),
                    &direct_token,
                    direct.method,
                    direct.path,
                    &direct_body,
                )
                .await;
                assert_eq!(
                    direct_status,
                    StatusCode::OK,
                    "{op:?} direct {} {} failed: {direct_resp}",
                    direct.method,
                    direct.path
                );
                assert!(
                    direct_resp.get("error").is_none(),
                    "{op:?} direct RPC refused: {direct_resp}"
                );
            }
        }
    }

    // Verify router also refuses an Agent calling PlanRpc directly over /api/mesh/rpc.
    let agent_caller = CallerContext {
        origin: AuthenticatedOrigin::Agent,
        agent: Some(AuthenticatedAgent {
            instance_id: "source".into(),
            tab_id: "worker-tab-1".into(),
            can_spawn: false,
        }),
        expires_at_ms: None,
    };
    let forged_token = sign_for_caller(&source, &agent_caller);
    let (status, refused) = send_signed(
        target_app.clone(),
        &forged_token,
        "POST",
        "/api/mesh/rpc",
        &json!({
            "method": "plan.get",
            "request_id": "forged-agent-plan",
            "protocol_version": 1,
            "params": { "plan_id": "pl_1" }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refused["error"]["code"], -32003, "{refused}");

    // Verify a local non-agent caller cannot spoof source_peer to bypass live-agent admission.
    let (spoof_status, _) = send_signed(
        target_app.clone(),
        "owner-secret",
        "POST",
        "/api/notifications",
        &json!({
            "title": "Spoofed peer notice",
            "source_peer": "source"
        }),
    )
    .await;
    assert_eq!(spoof_status, StatusCode::BAD_REQUEST);

    let _ = stop.send(());
    let _ = server.await;
}

async fn send_relayed_notification(
    app: axum::Router,
    source: &AppState,
    caller: &CallerContext,
    body: Value,
) -> (StatusCode, Value) {
    let token = sign_for_caller(source, caller);
    send_signed(
        app,
        &token,
        "POST",
        "/api/mesh/relay",
        &json!({
            "method": "POST",
            "path": "/api/notifications",
            "body": body,
        }),
    )
    .await
}

#[tokio::test]
async fn remote_notification_admits_owner_and_mesh_for_full_account_peer_and_denies_remote_agent() {
    initialize();

    let source_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = peer_state(&source_fixture, "source-origin-test", 0);
    let target = peer_state(&target_fixture, "target-origin-test", 0);
    pair_with_access(&target, &source, Some("Cloud Box"), AccessLevel::Full);
    let app = routes::create_router_with_frontend(target.clone(), None);

    for (origin, title) in [
        (AuthenticatedOrigin::Owner, "Owner remote notice"),
        (AuthenticatedOrigin::Mesh, "Mesh remote notice"),
    ] {
        let caller = CallerContext {
            origin,
            agent: None,
            expires_at_ms: None,
        };
        let (status, response) = send_relayed_notification(
            app.clone(),
            &source,
            &caller,
            json!({
                "title": title,
                "body": "display only",
                "severity": "info",
                "level": "verbose"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{origin:?}: {response}");
        assert_eq!(response["ok"], true, "{origin:?}: {response}");
        assert_eq!(response["status"], 200, "{origin:?}: {response}");
    }

    let agent_caller = CallerContext {
        origin: AuthenticatedOrigin::Agent,
        agent: Some(AuthenticatedAgent {
            instance_id: "source-origin-test".into(),
            tab_id: "worker-tab-1".into(),
            can_spawn: false,
        }),
        expires_at_ms: None,
    };
    let (status, denied) = send_relayed_notification(
        app.clone(),
        &source,
        &agent_caller,
        json!({ "title": "Remote worker notice" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{denied}");
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["status"], 403, "{denied}");
    assert_eq!(denied["data"]["error"]["bytes_written"], false, "{denied}");

    let notif = target.svc.notif.lock();
    let notifs = notif.list(false);
    assert_eq!(notifs.len(), 2, "only Owner and Mesh notifications stored");
    for n in &notifs {
        assert_eq!(
            n.peer_name.as_deref(),
            Some("Cloud Box"),
            "source_peer must come from authenticated paired peer"
        );
        assert!(n.tab_id.is_none());
        assert!(n.workspace_id.is_none());
        assert!(n.target_tab_id.is_none());
        assert!(n.chat_message_id.is_none());
        assert!(n.actions.is_none());
        assert!(n.surface_offer.is_none());
        assert!(n.answer.is_none());
    }
}

#[tokio::test]
async fn remote_notification_requires_full_access_and_direct_route_keeps_actor_headers() {
    initialize();

    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let target = peer_state(&target_fixture, "target-access-test", 0);

    let full_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let full_source = peer_state(&full_fixture, "source-full-direct-test", 0);
    pair_with_access(&target, &full_source, Some("Full Peer"), AccessLevel::Full);

    let ro_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let ro_source = peer_state(&ro_fixture, "source-ro-test", 0);
    pair_with_access(&target, &ro_source, Some("RO Peer"), AccessLevel::ReadOnly);

    let iso_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let iso_source = peer_state(&iso_fixture, "source-iso-test", 0);
    pair_with_access(
        &target,
        &iso_source,
        Some("Iso Peer"),
        AccessLevel::AgentIsolated,
    );

    let custom_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let custom_source = peer_state(&custom_fixture, "source-custom-admin-test", 0);
    pair_with_access(
        &target,
        &custom_source,
        Some("Custom Admin"),
        AccessLevel::Custom(vec![Permission::Admin]),
    );

    let app = routes::create_router_with_frontend(target.clone(), None);
    let mesh_caller = CallerContext {
        origin: AuthenticatedOrigin::Mesh,
        agent: None,
        expires_at_ms: None,
    };

    for (label, peer) in [
        ("ReadOnly", &ro_source),
        ("AgentIsolated", &iso_source),
        ("Custom(Admin)", &custom_source),
    ] {
        let (status, response) = send_relayed_notification(
            app.clone(),
            peer,
            &mesh_caller,
            json!({ "title": format!("{label} attempt") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{label}: {response}");
        assert_eq!(response["ok"], false, "{label}: {response}");
        assert_eq!(response["status"], 403, "{label}: {response}");
    }

    // Direct POST /api/notifications stays on ActorHeaders, refusing remote peer callers.
    for caller in [
        CallerContext {
            origin: AuthenticatedOrigin::Owner,
            agent: None,
            expires_at_ms: None,
        },
        mesh_caller,
        CallerContext {
            origin: AuthenticatedOrigin::Agent,
            agent: Some(AuthenticatedAgent {
                instance_id: "source-full-direct-test".into(),
                tab_id: "worker-tab-1".into(),
                can_spawn: false,
            }),
            expires_at_ms: None,
        },
    ] {
        let token = sign_for_caller(&full_source, &caller);
        let (status, _) = send_signed(
            app.clone(),
            &token,
            "POST",
            "/api/notifications",
            &json!({ "title": "Direct remote attempt" }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "direct /api/notifications must stay on ActorHeaders for {:?}",
            caller.origin
        );
    }

    assert!(target.svc.notif.lock().list(false).is_empty());
}

#[tokio::test]
async fn remote_notification_rejects_navigation_offer_actions_and_source_peer_fields() {
    initialize();

    let source_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = peer_state(&source_fixture, "source-fields-test", 0);
    let target = peer_state(&target_fixture, "target-fields-test", 0);
    pair_with_access(&target, &source, Some("Cloud Box"), AccessLevel::Full);
    let app = routes::create_router_with_frontend(target.clone(), None);
    let caller = CallerContext {
        origin: AuthenticatedOrigin::Mesh,
        agent: None,
        expires_at_ms: None,
    };

    for (label, payload) in [
        (
            "source_peer",
            json!({ "title": "Notice", "source_peer": "forged-peer" }),
        ),
        (
            "tab_id",
            json!({ "title": "Notice", "tab_id": "local-tab" }),
        ),
        (
            "workspace_id",
            json!({ "title": "Notice", "workspace_id": "local-ws" }),
        ),
        (
            "target_tab_id",
            json!({ "title": "Notice", "target_tab_id": "local-tab" }),
        ),
        (
            "chat_message_id",
            json!({ "title": "Notice", "chat_message_id": "msg-1" }),
        ),
        (
            "offer",
            json!({
                "title": "Notice",
                "offer": "desktop",
                "offer_accept": "Enable",
                "level": "action_needed"
            }),
        ),
        (
            "offer_accept",
            json!({ "title": "Notice", "offer_accept": "Enable" }),
        ),
        (
            "actions",
            json!({
                "title": "Notice",
                "actions": [{ "type": "answer", "label": "Approve" }]
            }),
        ),
    ] {
        let (status, response) =
            send_relayed_notification(app.clone(), &source, &caller, payload).await;
        assert_eq!(status, StatusCode::OK, "{label}: {response}");
        assert_eq!(response["ok"], false, "{label}: {response}");
        assert_eq!(response["status"], 400, "{label}: {response}");
        assert_eq!(
            response["data"]["error"]["code"], REMOTE_NOTIFICATION_REJECTED,
            "{label}: {response}"
        );
        assert_eq!(
            response["data"]["error"]["bytes_written"], false,
            "{label}: {response}"
        );
    }

    assert!(target.svc.notif.lock().list(false).is_empty());
}

#[tokio::test]
async fn remote_notification_enforces_per_peer_rate_limit_independently() {
    initialize();

    let peer_a_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let peer_b_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let peer_a = peer_state(&peer_a_fixture, "source-rate-limit-a", 0);
    let peer_b = peer_state(&peer_b_fixture, "source-rate-limit-b", 0);
    let target = peer_state(&target_fixture, "target-rate-limit", 0);
    pair_with_access(&target, &peer_a, Some("Cloud A"), AccessLevel::Full);
    pair_with_access(&target, &peer_b, Some("Cloud B"), AccessLevel::Full);
    let app = routes::create_router_with_frontend(target.clone(), None);
    let caller = CallerContext {
        origin: AuthenticatedOrigin::Mesh,
        agent: None,
        expires_at_ms: None,
    };

    for idx in 0..MAX_PER_PEER_NOTIFICATIONS {
        let (status, response) = send_relayed_notification(
            app.clone(),
            &peer_a,
            &caller,
            json!({ "title": format!("Peer A notice {idx}") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "slot {idx}: {response}");
        assert_eq!(response["ok"], true, "slot {idx}: {response}");
        assert_eq!(response["status"], 200, "slot {idx}: {response}");
    }

    let (status, limited) = send_relayed_notification(
        app.clone(),
        &peer_a,
        &caller,
        json!({ "title": "Peer A over quota" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{limited}");
    assert_eq!(limited["ok"], false, "{limited}");
    assert_eq!(limited["status"], 429, "{limited}");
    assert_eq!(
        limited["data"]["error"]["code"], PEER_NOTIFICATION_RATE_LIMITED,
        "{limited}"
    );
    assert_eq!(
        limited["data"]["error"]["bytes_written"], false,
        "{limited}"
    );

    let (status_b, response_b) = send_relayed_notification(
        app.clone(),
        &peer_b,
        &caller,
        json!({ "title": "Peer B unaffected" }),
    )
    .await;
    assert_eq!(status_b, StatusCode::OK, "{response_b}");
    assert_eq!(response_b["ok"], true, "{response_b}");
    assert_eq!(response_b["status"], 200, "{response_b}");
}
