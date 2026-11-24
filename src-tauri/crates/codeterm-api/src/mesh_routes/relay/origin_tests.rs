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
        domios_version: None,
        domios_build: None,
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
    let mesh = first.mesh.lock().sign_request().unwrap();
    let app = Router::new()
        .merge(crate::transport_capabilities::router())
        .route(
            "/api/surfaces/{tab_id}/call",
            post(crate::surface_routes::call),
        )
        .route("/api/mesh/relay-to", post(super::mesh_relay_to_peer))
        .layer(axum::middleware::from_fn_with_state(
            second.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(second.clone());
    let destination = || PeerClient::new_signed("127.0.0.1", 1, "discarded");
    for (label, token, expected) in [
        ("agent", agent, AuthenticatedOrigin::Agent),
        ("owner", "owner-secret".into(), AuthenticatedOrigin::Owner),
        ("mesh", mesh, AuthenticatedOrigin::Mesh),
    ] {
        let caller = headers(&token);
        let initiating_agent = crate::auth::authenticated_agent(&first, &caller).unwrap();
        for (method, path, body) in [
            ("GET", "/api/system/default-cwd".into(), Value::Null),
            (
                "POST",
                format!("/api/surfaces/{surface}/call"),
                json!({"method":"profiles.create","args":{"name":label}}),
            ),
            (
                "POST",
                "/api/mesh/relay-to".into(),
                json!({"target_peer_id":"first","method":"GET","path":"/api/system/default-cwd"}),
            ),
        ] {
            let cmd = RelayCommand {
                method: method.into(),
                path,
                body,
                delivery_id: None,
            };
            let client = origin::peer_client(&first, &caller, destination()).unwrap();
            let outward = client.relay_request(&cmd).unwrap();
            let authorization = outward.headers()["authorization"].clone();
            let loopback =
                origin::loopback_request_with_auth(&second, &cmd, &cmd.body, authorization.clone())
                    .unwrap();

            assert_eq!(loopback.headers()["authorization"], authorization);
            assert_eq!(
                authenticated_origin(&second, loopback.headers()).unwrap(),
                expected
            );
            assert_eq!(
                crate::auth::authenticated_agent(&second, loopback.headers()).unwrap(),
                initiating_agent,
            );
            assert!(matches!(
                crate::auth::caller_identity(&second, loopback.headers()),
                Ok(crate::auth::CallerIdentity::Peer { instance_id, .. }) if instance_id == "first"
            ));
            assert!(matches!(
                origin::peer_client(&second, loopback.headers(), destination()),
                Err((StatusCode::FORBIDDEN, _))
            ));

            let response = app.clone().oneshot(wire_request(loopback)).await.unwrap();
            if cmd.path == "/api/mesh/relay-to"
                || (method == "GET" && expected == AuthenticatedOrigin::Agent)
            {
                assert_eq!(
                    response.status(),
                    StatusCode::FORBIDDEN,
                    "{label} {}",
                    cmd.path
                );
            } else {
                assert_eq!(response.status(), StatusCode::OK, "{label} {}", cmd.path);
                if method == "POST" {
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
                        assert!(body.get("result").is_some(), "{label}: {body}");
                    }
                }
            }
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
    let refusal = perform_test_relay(
        &first,
        "second",
        &cmd,
        AuthenticatedOrigin::Agent,
        None,
        None,
    )
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
        let request = origin::loopback_request_with_auth(
            &second,
            &cmd,
            &cmd.body,
            outward.headers()["authorization"].clone(),
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

#[tokio::test]
async fn signed_peer_and_loopback_inventory_use_the_same_restricted_grant() {
    use axum::routing::get;
    use codeterm_service_core::service::store::ServiceCreate;

    let a = ServiceFixture::new(MultiplexerState::new().0);
    let b = ServiceFixture::new(MultiplexerState::new().0);
    let source = state(&a, "inventory-origin");
    let target = state(&b, "inventory-target");
    pair(&target, &source);
    assert!(target
        .mesh
        .lock()
        .set_peer_access("inventory-origin", AccessLevel::ReadOnly));
    let service: ServiceCreate = serde_json::from_value(json!({
        "name": "inventory service", "kind": "daemon", "health_mode": "http",
    }))
    .unwrap();
    let id = target.svc.services_store.create(service).unwrap();
    let token = origin::sign(&source, AuthenticatedOrigin::Owner, None).unwrap();
    let signed_headers = headers(&token);
    let app = Router::new()
        .route("/api/mesh/inventory", get(crate::mesh_inventory::inventory))
        .route(
            "/api/services/{id}",
            get(crate::services_routes::get_service).delete(crate::services_routes::delete_service),
        )
        .layer(axum::middleware::from_fn_with_state(
            target.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(target.clone());
    let cmd = RelayCommand {
        method: "GET".into(),
        path: "/api/mesh/inventory".into(),
        body: Value::Null,
        delivery_id: None,
    };
    let signed = origin::loopback_request_with_auth(
        &target,
        &cmd,
        &cmd.body,
        signed_headers["authorization"].clone(),
    )
    .unwrap();
    let forwarded = app.clone().oneshot(wire_request(signed)).await.unwrap();
    assert_eq!(forwarded.status(), StatusCode::OK);
    let forwarded: Value =
        serde_json::from_slice(&to_bytes(forwarded.into_body(), 65536).await.unwrap()).unwrap();
    let direct = app
        .clone()
        .oneshot(
            Request::get(cmd.path)
                .header("authorization", &signed_headers["authorization"])
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(direct.status(), StatusCode::OK);
    let direct: Value =
        serde_json::from_slice(&to_bytes(direct.into_body(), 65536).await.unwrap()).unwrap();

    for view in [direct, forwarded] {
        let capability = &view["resources"][0]["capabilities"];
        assert_eq!(view["resources"][0]["resource_id"], id);
        assert_eq!(capability["get"]["allowed"], true);
        assert_eq!(capability["delete"]["allowed"], false);
    }
    let trigger = RelayCommand {
        method: "POST".into(),
        path: "/api/crons/disposable-b/trigger".into(),
        body: Value::Null,
        delivery_id: None,
    };
    let refused = perform_test_relay(
        &target,
        "inventory-origin",
        &trigger,
        AuthenticatedOrigin::Owner,
        None,
        signed_headers.get("authorization"),
    )
    .await
    .unwrap();
    assert!(!refused.ok);
    assert_eq!(refused.status, 403);
    let error = &refused.data.as_ref().unwrap()["error"];
    assert_eq!(
        error["code"],
        codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED
    );
    assert_eq!(error["bytes_written"], false);

    for (method, status) in [
        (Method::GET, StatusCode::OK),
        (Method::DELETE, StatusCode::FORBIDDEN),
    ] {
        let answer = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/services/{id}"))
                    .header("authorization", &signed_headers["authorization"])
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(answer.status(), status);
    }
}

#[tokio::test]
async fn grant_change_fences_already_buffered_peer_events_and_isolated_resume_filters_replay() {
    use axum::routing::get;
    use futures_util::StreamExt;

    let a = ServiceFixture::new(MultiplexerState::new().0);
    let (mux, own_tab) = MultiplexerState::new();
    let b = ServiceFixture::new(mux);
    let source = state(&a, "rail-origin");
    let target = state(&b, "rail-target");
    pair(&target, &source);
    {
        let mut mux = target.svc.mux.write();
        mux.get_tab_mut(&own_tab).unwrap().ownership = Some(codeterm_types::pty::RemoteOwnership {
            controller_title: "owned".into(),
            controller_tab_id: None,
            peer_id: "rail-origin".into(),
            peer_name: "rail-origin".into(),
            team: None,
        });
    }
    let status = |tab: &str| crate::events::SystemEvent::Generic {
        event_type: "tab-status-changed".into(),
        data: json!({"tab_id":tab,"status":"Working"}),
    };
    target.events.publish(status(&own_tab));
    target.events.publish(status("another-owner"));
    target
        .events
        .publish(crate::events::SystemEvent::MuxStateChanged);

    let token = origin::sign(&source, AuthenticatedOrigin::Owner, None).unwrap();
    let app = Router::new()
        .route("/api/events/stream", get(crate::web_routes::events_stream))
        .layer(axum::middleware::from_fn_with_state(
            target.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(target.clone());
    let connect = || {
        Request::get(format!(
            "/api/events/stream?scope=peer&since={}:0",
            target.events.boot_id()
        ))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
    };

    let unscoped = app
        .clone()
        .oneshot(
            Request::get("/api/events/stream")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unscoped.status(), StatusCode::FORBIDDEN);

    let old = app.clone().oneshot(connect()).await.unwrap();
    assert_eq!(old.status(), StatusCode::OK);
    let mut body = old.into_body().into_data_stream();
    target.events.publish(status("queued-live"));
    {
        let mut mesh = target.mesh.lock();
        assert!(mesh.set_peer_access("rail-origin", AccessLevel::ReadOnly));
        assert!(mesh.set_peer_access("rail-origin", AccessLevel::Full));
    }
    assert!(body.next().await.is_none());

    assert!(target
        .mesh
        .lock()
        .set_peer_access("rail-origin", AccessLevel::AgentIsolated));
    let new = app.oneshot(connect()).await.unwrap();
    assert_eq!(new.status(), StatusCode::OK);
    let mut body = new.into_body().into_data_stream();
    let first = body.next().await.unwrap().unwrap();
    let frame: Value = serde_json::from_slice(first.strip_prefix(b"data: ").unwrap()).unwrap();
    assert_eq!(frame["data"]["tab_id"], own_tab);

    let boundary = body.next().await.unwrap().unwrap();
    let frame: Value = serde_json::from_slice(boundary.strip_prefix(b"data: ").unwrap()).unwrap();
    assert_eq!(frame["event_type"], "peer-replay-complete");
    target.events.publish(status("another-owner"));
    target.events.publish(status(&own_tab));
    let live = body.next().await.unwrap().unwrap();
    let frame: Value = serde_json::from_slice(live.strip_prefix(b"data: ").unwrap()).unwrap();
    assert_eq!(frame["data"]["tab_id"], own_tab);
}

#[test]
fn single_pass_signed_mesh_auth_enforces_origin_agent_binding_and_replay() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = state(&fixture, "source");
    let target = state(&fixture, "target");
    pair(&target, &source);

    let owner_token = origin::sign(&source, AuthenticatedOrigin::Owner, None).unwrap();
    let caller = origin::authenticate_signed_mesh_request(&target, &headers(&owner_token)).unwrap();
    assert_eq!(caller.peer_id, "source");
    assert_eq!(caller.origin, AuthenticatedOrigin::Owner);
    assert!(caller.agent.is_none());
    assert!(caller.grant.unwrap().current(&target.mesh.lock()));
    let replay = origin::authenticate_signed_mesh_request(&target, &headers(&owner_token));
    assert_eq!(replay.unwrap_err().0, StatusCode::UNAUTHORIZED);

    let claimed = codeterm_service_core::mesh::crown::AuthenticatedAgent {
        instance_id: "source".into(),
        tab_id: "tab-1".into(),
        can_spawn: false,
    };
    let agent_token = origin::sign(&source, AuthenticatedOrigin::Agent, Some(&claimed)).unwrap();
    let caller = origin::authenticate_signed_mesh_request(&target, &headers(&agent_token)).unwrap();
    assert_eq!(caller.peer_id, "source");
    assert_eq!(caller.origin, AuthenticatedOrigin::Agent);
    assert_eq!(caller.agent, Some(claimed));

    let spoofed = codeterm_service_core::mesh::crown::AuthenticatedAgent {
        instance_id: "other".into(),
        tab_id: "tab-1".into(),
        can_spawn: false,
    };
    let spoofed_token = origin::sign(&source, AuthenticatedOrigin::Agent, Some(&spoofed)).unwrap();
    assert_eq!(
        origin::authenticate_signed_mesh_request(&target, &headers(&spoofed_token))
            .unwrap_err()
            .0,
        StatusCode::UNAUTHORIZED
    );

    let local_token = origin::sign(&target, AuthenticatedOrigin::Owner, None).unwrap();
    let local = origin::authenticate_signed_mesh_request(&target, &headers(&local_token)).unwrap();
    assert_eq!(local.peer_id, "local");
    assert_eq!(local.origin, AuthenticatedOrigin::Owner);
    assert!(local.agent.is_none());
    assert!(local.grant.is_none());
}

#[test]
fn owner_tab_close_revalidates_verified_grant_in_blocking_worker() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
        .block_on(async {
            let (mut mux, initial) = MultiplexerState::new();
            let workspace = mux.find_workspace_for_tab(&initial).unwrap();
            let tab = mux.create_tab(&workspace).unwrap();
            let fixture = ServiceFixture::new(mux);
            let source = state(&fixture, "source");
            let target = state(&fixture, "target");
            pair(&target, &source);
            let token = origin::sign(&source, AuthenticatedOrigin::Owner, None).unwrap();
            let caller =
                origin::authenticate_signed_mesh_request(&target, &headers(&token)).unwrap();
            let cmd = RelayCommand {
                method: "POST".into(),
                path: format!("/api/tabs/{tab}/close"),
                body: json!({"as_owner": true}),
                delivery_id: None,
            };
            let (entered, blocked) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                entered.send(()).unwrap();
                let _ = wait.recv();
            });
            blocked.await.unwrap();

            let mut close = std::pin::pin!(perform_relay(&target, &caller, &cmd, None));
            assert!(
                std::future::poll_fn(|cx| {
                    std::task::Poll::Ready(
                        std::future::Future::poll(close.as_mut(), cx).is_pending(),
                    )
                })
                .await
            );
            assert!(target
                .mesh
                .lock()
                .set_peer_access("source", AccessLevel::ReadOnly));
            release.send(()).unwrap();
            blocker.await.unwrap();

            let refused = close.await.unwrap();
            assert_eq!(refused.status, StatusCode::FORBIDDEN.as_u16());
            assert!(refused
                .error
                .unwrap()
                .contains("Peer authorization changed"));
            assert!(target.svc.mux.read().get_tab(&tab).is_some());

            assert!(target
                .mesh
                .lock()
                .set_peer_access("source", AccessLevel::Full));
            let stale = perform_relay(&target, &caller, &cmd, None).await.unwrap();
            assert_eq!(stale.status, StatusCode::FORBIDDEN.as_u16());
            assert!(target.svc.mux.read().get_tab(&tab).is_some());
        });
}

#[tokio::test]
async fn workspace_close_refuses_a_stale_grant_before_cascade_admission() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("SharedCloseFixture".into(), None).id;
    let tab = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let source = state(&fixture, "source");
    let target = state(&fixture, "target");
    pair(&target, &source);
    let token = origin::sign(&source, AuthenticatedOrigin::Owner, None).unwrap();
    let caller = origin::authenticate_signed_mesh_request(&target, &headers(&token)).unwrap();
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::ReadOnly));
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::Full));

    let refused =
        close_workspace_from_mesh_peer(&target, "source", &workspace, true, caller.grant.unwrap())
            .await
            .unwrap();
    assert_eq!(refused.status, StatusCode::FORBIDDEN.as_u16());
    assert!(refused
        .error
        .unwrap()
        .contains("Peer authorization changed"));
    let mux = target.svc.mux.read();
    assert!(mux.get_workspace(&workspace).is_some());
    assert!(mux.get_tab(&tab).is_some());
}
