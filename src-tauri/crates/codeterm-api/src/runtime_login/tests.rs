use super::*;
use axum::{body::Body, http::Request, middleware};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[tokio::test]
async fn runtime_login_read_requires_authentication_and_never_creates_a_runtime() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("test-token".into());
    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/tabs/{tab}/agent/runtime/login"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tabs/{tab}/agent/runtime/login"))
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .agent_runtime_state
        .is_none());
}

#[test]
fn runtime_login_response_preserves_opaque_values_without_http_caching() {
    let value = serde_json::json!({"login":{"status":"pending","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":5000}});
    let (headers, Json(body)) = login_response(value);
    assert_eq!(headers[axum::http::header::CACHE_CONTROL], "no-store");
    assert_eq!(body["login"]["url"], "https://example.invalid/opaque-path");
    assert_eq!(body["login"]["userCode"], "opaque-token");
    assert_eq!(body["login"]["expiresAtMs"], 5000);
}

#[tokio::test]
async fn agents_cannot_change_a_mode_or_host_approvals_and_the_owner_can() {
    let (mut mux, tab) = MultiplexerState::new();
    {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("fixture".into());
        target.agent_session_id = Some("own-thread".into());
        target.session_binding_nonce = Some("own-nonce".into());
        target.reported_session_binding_nonce = Some("own-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());
    let agent = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);
    let status = |token: String, path: String, body: serde_json::Value| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }
    };
    let controls = format!("/api/tabs/{tab}/agent/runtime/controls");
    let approvals = format!("/api/tabs/{tab}/agent/runtime/host-approvals");
    let selection = |mode: bool| {
        serde_json::json!({"threadId":"own-thread","bindingNonce":"own-nonce","epoch":1,
            "id":"opaque-control","value":"opaque-approve-all","mode":mode})
    };
    let allow = serde_json::json!({"allowAll":true});

    assert_eq!(
        status(agent.clone(), controls.clone(), selection(true)).await,
        StatusCode::FORBIDDEN,
        "an agent must not select a mode for itself"
    );
    assert_eq!(
        status(agent.clone(), approvals.clone(), allow.clone()).await,
        StatusCode::FORBIDDEN,
        "an agent must not lift its own Ask"
    );
    // Past the authority check, both reach the service and find no runtime or capability.
    assert_eq!(
        status(agent, controls.clone(), selection(false)).await,
        StatusCode::CONFLICT,
        "other controls stay open to agents"
    );
    assert_eq!(
        status("owner-token".into(), controls, selection(true)).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        status("owner-token".into(), approvals, allow).await,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn runtime_login_get_and_all_post_actions_require_owner_or_full_peer_authority() {
    use codeterm_service_core::mesh::{
        crown::{
            generate_peer_keypair, sign_request_with_agent, sign_request_with_origin,
            AuthenticatedAgent, AuthenticatedOrigin,
        },
        types::{AccessLevel, PairedPeer, Permission, RELAY_PROTOCOL_VERSION},
    };

    let (mut mux, tab) = MultiplexerState::new();
    {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("fixture".into());
        target.agent_session_id = Some("own-thread".into());
        target.session_binding_nonce = Some("own-nonce".into());
        target.reported_session_binding_nonce = Some("own-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());

    let agent_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();

    let (admin_priv, admin_pub) = generate_peer_keypair();
    let (full_priv, full_pub) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(PairedPeer {
            instance_id: "peer-custom-admin".into(),
            instance_name: "CustomAdminPeer".into(),
            addresses: vec![],
            api_port: 7685,
            public_key: admin_pub,
            paired_at: "now".into(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: AccessLevel::Custom(vec![Permission::Admin]),
            domios_version: None,
            domios_build: None,
            web_ui: None,
            relay_protocol_version: RELAY_PROTOCOL_VERSION,
            consecutive_misses: 0,
            last_active_ms: None,
        });
        mesh.insert_account_peer_for_test(PairedPeer {
            instance_id: "peer-full".into(),
            instance_name: "FullPeer".into(),
            addresses: vec![],
            api_port: 7685,
            public_key: full_pub,
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

    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);

    let login_path = format!("/api/tabs/{tab}/agent/runtime/login");
    let get_status = |token: String| {
        let app = app.clone();
        let path = login_path.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("GET")
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }
    };
    let post_status = |token: String, body: serde_json::Value| {
        let app = app.clone();
        let path = login_path.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }
    };

    // Local agent tab is refused on GET and every POST login action.
    assert_eq!(get_status(agent_token.clone()).await, StatusCode::FORBIDDEN);
    for body in [
        serde_json::json!({"action": "start"}),
        serde_json::json!({"action": "cancel"}),
        serde_json::json!({"action": "terminal"}),
        serde_json::json!({"action": "notify"}),
        serde_json::json!({"action": "code", "code": "opaque"}),
        serde_json::json!({"action": "backupCredentials"}),
        serde_json::json!({"action": "api_key", "apiKey": "opaque"}),
    ] {
        assert_eq!(
            post_status(agent_token.clone(), body.clone()).await,
            StatusCode::FORBIDDEN,
            "agent tab must be refused on login POST {body}"
        );
    }

    // Custom(Admin) peer passes allows_full_dispatch, so it must still be refused on GET and POST.
    let custom_admin_owner =
        sign_request_with_origin(&admin_priv, "peer-custom-admin", AuthenticatedOrigin::Owner)
            .unwrap();
    assert_eq!(get_status(custom_admin_owner).await, StatusCode::FORBIDDEN);
    for action in ["start", "cancel", "terminal", "notify", "backupCredentials"] {
        let token =
            sign_request_with_origin(&admin_priv, "peer-custom-admin", AuthenticatedOrigin::Owner)
                .unwrap();
        assert_eq!(
            post_status(token, serde_json::json!({"action": action})).await,
            StatusCode::FORBIDDEN,
            "Custom(Admin) peer must be refused on login POST action {action}"
        );
    }

    // Full peer carrying an Agent origin must be refused on GET and POST.
    let full_agent = AuthenticatedAgent {
        instance_id: "peer-full".into(),
        tab_id: "remote-agent-tab".into(),
        can_spawn: false,
    };
    let full_agent_get = sign_request_with_agent(
        &full_priv,
        "peer-full",
        AuthenticatedOrigin::Agent,
        Some(&full_agent),
    )
    .unwrap();
    assert_eq!(get_status(full_agent_get).await, StatusCode::FORBIDDEN);
    let full_agent_post = sign_request_with_agent(
        &full_priv,
        "peer-full",
        AuthenticatedOrigin::Agent,
        Some(&full_agent),
    )
    .unwrap();
    assert_eq!(
        post_status(full_agent_post, serde_json::json!({"action": "start"})).await,
        StatusCode::FORBIDDEN
    );

    // Full peer with Owner or Mesh origin passes authority check (reaches 409 without a runtime).
    let full_owner_get =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    assert_eq!(get_status(full_owner_get).await, StatusCode::CONFLICT);
    let full_mesh_post =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Mesh).unwrap();
    assert_eq!(
        post_status(full_mesh_post, serde_json::json!({"action": "start"})).await,
        StatusCode::CONFLICT
    );
    // Remote Full peer is still refused on local-only api_key submission.
    let full_owner_key =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    assert_eq!(
        post_status(
            full_owner_key,
            serde_json::json!({"action": "api_key", "apiKey": "opaque"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn code_submission_requires_owner_or_full_peer_owner_origin_and_exact_route_binding() {
    use codeterm_service_core::mesh::{
        crown::{
            generate_peer_keypair, sign_request_with_agent, sign_request_with_origin,
            AuthenticatedAgent, AuthenticatedOrigin,
        },
        types::{AccessLevel, PairedPeer, Permission, RELAY_PROTOCOL_VERSION},
    };

    let (mut mux, tab) = MultiplexerState::new();
    {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("fixture".into());
        target.agent_session_id = Some("own-session".into());
        target.session_binding_nonce = Some("own-nonce".into());
        target.reported_session_binding_nonce = Some("own-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());
    let own_installation = state.mesh.lock().ident.identity.instance_id.clone();

    let agent_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();

    let (full_priv, full_pub) = generate_peer_keypair();
    let (admin_priv, admin_pub) = generate_peer_keypair();
    let (readonly_priv, readonly_pub) = generate_peer_keypair();
    let (isolated_priv, isolated_pub) = generate_peer_keypair();
    let (forged_priv, _forged_pub) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        for (id, name, public_key, access_level) in [
            ("peer-full", "FullPeer", full_pub, AccessLevel::Full),
            (
                "peer-admin",
                "AdminPeer",
                admin_pub,
                AccessLevel::Custom(vec![Permission::Admin]),
            ),
            (
                "peer-readonly",
                "ReadOnlyPeer",
                readonly_pub,
                AccessLevel::ReadOnly,
            ),
            (
                "peer-isolated",
                "IsolatedPeer",
                isolated_pub,
                AccessLevel::AgentIsolated,
            ),
        ] {
            mesh.insert_account_peer_for_test(PairedPeer {
                instance_id: id.into(),
                instance_name: name.into(),
                addresses: vec![],
                api_port: 7685,
                public_key,
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
    }

    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);

    let login_path = format!("/api/tabs/{tab}/agent/runtime/login");
    let valid_code_body = serde_json::json!({
        "action": "code",
        "code": "opaque-auth-code",
        "loginId": "active-flow-1",
        "providerId": "fixture",
        "binding": {
            "installationId": own_installation,
            "tabId": tab,
            "providerId": "fixture",
            "sessionId": "own-session",
            "nonce": "own-nonce",
            "epoch": 1,
        }
    });

    let post = |token: Option<String>, body: serde_json::Value| {
        let app = app.clone();
        let path = login_path.clone();
        async move {
            let mut builder = Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json");
            if let Some(token) = token {
                builder = builder.header("authorization", format!("Bearer {token}"));
            }
            let response = app
                .oneshot(builder.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 64)
                .await
                .unwrap();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            (status, text)
        }
    };

    // Unauthenticated & forged callers are 401 UNAUTHORIZED.
    let (status, _) = post(None, valid_code_body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = post(Some("forged-bearer-token".into()), valid_code_body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let forged_peer =
        sign_request_with_origin(&forged_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    let (status, _) = post(Some(forged_peer), valid_code_body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Local agent tab, remote Full+Agent, remote Full+Mesh, Custom(Admin), ReadOnly, AgentIsolated are ALL 403 FORBIDDEN.
    let full_agent = AuthenticatedAgent {
        instance_id: "peer-full".into(),
        tab_id: "worker-tab".into(),
        can_spawn: false,
    };
    for (label, token) in [
        ("local agent tab", agent_token),
        (
            "full peer with agent origin",
            sign_request_with_agent(
                &full_priv,
                "peer-full",
                AuthenticatedOrigin::Agent,
                Some(&full_agent),
            )
            .unwrap(),
        ),
        (
            "full peer with mesh origin",
            sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Mesh).unwrap(),
        ),
        (
            "custom admin peer with owner origin",
            sign_request_with_origin(&admin_priv, "peer-admin", AuthenticatedOrigin::Owner)
                .unwrap(),
        ),
        (
            "readonly peer with owner origin",
            sign_request_with_origin(&readonly_priv, "peer-readonly", AuthenticatedOrigin::Owner)
                .unwrap(),
        ),
        (
            "isolated peer with owner origin",
            sign_request_with_origin(&isolated_priv, "peer-isolated", AuthenticatedOrigin::Owner)
                .unwrap(),
        ),
    ] {
        let (status, body) = post(Some(token), valid_code_body.clone()).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{label} must be refused on code submission: {body}"
        );
        assert!(
            !body.contains("opaque-auth-code"),
            "{label} refusal must never echo the submitted code"
        );
    }

    // Local Owner and Full peer with Owner origin pass authority & route binding (reach service 409).
    let (status, body) = post(Some("owner-token".into()), valid_code_body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["outcome"], "refused");
    assert_eq!(parsed["error"]["bytes_written"], false);

    let full_owner =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    let (status, body) = post(Some(full_owner), valid_code_body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["outcome"], "refused");
    assert!(!body.contains("opaque-auth-code"));

    // Remote Full+Owner with missing/mismatched installation, tab, provider, or loginId is refused at route boundary.
    for (label, bad_body) in [
        (
            "mismatched installationId",
            serde_json::json!({
                "action": "code", "code": "opaque-auth-code", "loginId": "active-flow-1", "providerId": "fixture",
                "binding": {"installationId": "foreign-inst", "tabId": tab, "providerId": "fixture", "nonce": "own-nonce", "epoch": 1}
            }),
        ),
        (
            "missing installationId",
            serde_json::json!({
                "action": "code", "code": "opaque-auth-code", "loginId": "active-flow-1", "providerId": "fixture",
                "binding": {"tabId": tab, "providerId": "fixture", "nonce": "own-nonce", "epoch": 1}
            }),
        ),
        (
            "mismatched tabId",
            serde_json::json!({
                "action": "code", "code": "opaque-auth-code", "loginId": "active-flow-1", "providerId": "fixture",
                "binding": {"installationId": own_installation, "tabId": "foreign-tab", "providerId": "fixture", "nonce": "own-nonce", "epoch": 1}
            }),
        ),
        (
            "missing loginId",
            serde_json::json!({
                "action": "code", "code": "opaque-auth-code", "providerId": "fixture",
                "binding": {"installationId": own_installation, "tabId": tab, "providerId": "fixture", "nonce": "own-nonce", "epoch": 1}
            }),
        ),
        (
            "missing binding",
            serde_json::json!({
                "action": "code", "code": "opaque-auth-code", "loginId": "active-flow-1", "providerId": "fixture"
            }),
        ),
        (
            "missing providerId",
            serde_json::json!({
                "action": "code", "code": "opaque-auth-code", "loginId": "active-flow-1",
                "binding": {"installationId": own_installation, "tabId": tab, "nonce": "own-nonce", "epoch": 1}
            }),
        ),
    ] {
        let token =
            sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
        let (status, body) = post(Some(token), bad_body).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{label} must be refused at route binding check"
        );
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["outcome"], "refused", "{label}: {body}");
        assert_eq!(
            parsed["error"]["code"], "login_binding_mismatch",
            "{label}: {body}"
        );
        assert!(!body.contains("opaque-auth-code"));
    }
}
