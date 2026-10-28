use super::*;

#[tokio::test]
async fn granted_workspace_spawn_still_refuses_unsafe_launch_flags_before_creating_a_child() {
    use axum::{body::Body, http::Request, routing::post, Router};
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(true).await;
    let workspace = state
        .svc
        .mux
        .read()
        .find_workspace_for_tab(&concierge)
        .unwrap();
    fixture.seed_neighbor_concierge(&concierge,&"a".repeat(64),serde_json::from_value(serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":true,"workspaces":[workspace]})).unwrap()).await;
    let before = state.svc.mux.read().snapshot().tabs.len();
    let router = Router::new()
        .route("/api/agents/spawn", post(crate::agent_routes::spawn_agent))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let response = router.oneshot(Request::builder().method("POST").uri("/api/agents/spawn").header("authorization",format!("Bearer {token}")).header("content-type","application/json").body(Body::from(serde_json::json!({"agent":"missing-provider","workspace_id":workspace,"cwd":fixture.temp.path(),"args":["opaque-option"]}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], "neighbor_capability_denied");
    assert_eq!(state.svc.mux.read().snapshot().tabs.len(), before);
}

#[tokio::test]
async fn workspace_http_boundary_filters_listing_and_refuses_foreign_or_raw_sends() {
    use axum::{
        body::Body,
        http::Request,
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(true).await;
    let (workspace, allowed, shell) = {
        let mut mux = state.svc.mux.write();
        let workspace = mux.create_workspace("opaque-project".into(), None).id;
        let allowed = mux.create_tab(&workspace).unwrap();
        mux.get_tab_mut(&allowed).unwrap().agent_type = Some("fixture".into());
        let shell = mux.create_tab(&workspace).unwrap();
        (workspace, allowed, shell)
    };
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                serde_json::json!({"allowed":true,"scope":"opaque","workspaces":[workspace]}),
            )
            .unwrap(),
        )
        .await;
    fixture.install_seeded_storage_desk();
    let router = Router::new()
        .route("/api/tabs", get(crate::tab_routes::list_tabs))
        .route("/api/tabs/{id}/send", post(crate::tab_routes::send_text))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tabs?all_workspaces=true")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["id"], allowed);
    assert!(rows[0].get("session_binding_nonce").is_none());
    for (target, body) in [
        (&allowed, serde_json::json!({"text":"opaque","raw":true})),
        (&shell, serde_json::json!({"text":"opaque"})),
        (&concierge, serde_json::json!({"text":"opaque"})),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tabs/{target}/send"))
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn concierge_capability_refusals_keep_typed_http_403() {
    for reason in [
        "neighbor_memory_not_allowed",
        "neighbor_memory_read_only",
        "neighbor_secrets_not_allowed",
        "neighbor_files_not_allowed",
        "neighbor_capability_denied",
        "neighbor_memory_area_denied",
        "tab_requests_not_allowed",
    ] {
        let response = neighbor_capability_failure(format!("{reason}: opaque"));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["reason"], reason);
        assert_eq!(value["code"], reason);
    }
}

async fn concierge_fixture(
    ready: bool,
) -> (
    codeterm_service_core::service::test_fixture::ServiceFixture,
    AppState,
    String,
    String,
) {
    let (mut mux, general) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = ready.then(|| "own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = ready.then(|| "own-nonce".into());
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
            )
            .unwrap(),
        )
        .await;
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    let token = state
        .svc
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (fixture, state, concierge, token)
}

#[tokio::test]
async fn authenticated_http_allowlist_runs_before_every_route_and_custom_auth_exemption() {
    use axum::{body::Body, http::Request, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let (_fixture, state, concierge, token) = concierge_fixture(true).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let downstream = calls.clone();
    let router = Router::new()
        .fallback(move || {
            let calls = downstream.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for (method, path) in [
        ("GET", "/api/tabs/foreign/text"),
        ("POST", "/api/tabs/foreign/send"),
        ("PATCH", "/api/config"),
        ("GET", "/api/cron"),
        ("POST", "/api/mesh/relay"),
        ("POST", "/api/mesh/relay-to"),
        ("GET", "/api/mesh/relay-asset/opaque"),
        ("POST", "/api/notifications"),
        ("POST", "/api/plugins/opaque/run"),
        ("POST", "/api/bridge/send"),
        ("GET", "/api/secrets/opaque"),
        ("POST", "/api/mem/items"),
        ("POST", "/api/tabs"),
        ("POST", "/api/tabs/foreign/prompt/respond"),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "downstream called: {path}");
    }
    for path in [
        "/api/mem/search".to_owned(),
        format!("/api/tabs/{concierge}/agent/runtime/questions/opaque/answer"),
    ] {
        let method = if path.contains("/questions/") {
            "POST"
        } else {
            "GET"
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let request = Request::builder()
        .uri("/api/mem/search")
        .header("authorization", format!("Bearer {token}"))
        .header("x-codeterm-tab-id", "foreign")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn own_launch_hooks_work_before_attestation_but_foreign_hooks_and_self_approval_stay_denied()
{
    use axum::{body::Body, http::Request, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(false).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let downstream = calls.clone();
    let router = Router::new()
        .fallback(move || {
            let calls = downstream.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for event in ["session_start", "stop", "context", "permission"] {
        for (target, status) in [
            (concierge.as_str(), StatusCode::NO_CONTENT),
            ("foreign", StatusCode::FORBIDDEN),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(format!("/api/tabs/{target}/hook/{event}"))
                .header("authorization", format!("Bearer {token}"))
                .header("x-codeterm-binding-nonce", "own-nonce")
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                status
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let wrong_nonce = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{concierge}/hook/session_start"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-codeterm-binding-nonce", "foreign-nonce")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(wrong_nonce).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&concierge).unwrap();
        tab.agent_session_id = Some("own-session".into());
        tab.reported_session_binding_nonce = Some("own-nonce".into());
    }
    for path in [
        format!("/api/tabs/{concierge}/prompt/respond"),
        format!("/api/tabs/{concierge}/hook/unknown"),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn real_history_route_refuses_other_friendships_without_identity_headers() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let (_fixture, state, _, token) = concierge_fixture(true).await;
    let router = crate::routes::create_router(state);
    for (friendship, status) in [
        ("a".repeat(64), StatusCode::OK),
        ("b".repeat(64), StatusCode::FORBIDDEN),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/api/account/neighbors/runtime")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"operation":"history","friendship_id":friendship,"limit":20})
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            status
        );
    }
}

#[tokio::test]
async fn a_ready_credential_is_revoked_when_its_session_is_removed_before_a_hook() {
    use axum::{body::Body, http::Request, Router};
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(true).await;
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&concierge)
        .unwrap()
        .agent_session_id = None;
    let router = Router::new()
        .fallback(|| async { StatusCode::NO_CONTENT })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{concierge}/hook/session_start"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-codeterm-binding-nonce", "own-nonce")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn own_todo_read_is_intrinsic_but_foreign_todos_never_reach_http_dispatch() {
    use axum::{body::Body, http::Request, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let (_fixture, state, concierge, token) = concierge_fixture(false).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let downstream = calls.clone();
    let router = Router::new()
        .fallback(move || {
            let calls = downstream.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for (target, expected) in [
        (concierge.as_str(), StatusCode::NO_CONTENT),
        ("foreign", StatusCode::FORBIDDEN),
    ] {
        let request = Request::builder()
            .uri(format!("/api/tabs/{target}/todos"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn secrets_http_refuses_neighbor_before_any_secret_backend_handler() {
    use axum::{body::Body, http::Request, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let (_fixture, state, _, token) = concierge_fixture(true).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let downstream = calls.clone();
    let router = Router::new()
        .fallback(move || {
            let calls = downstream.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for path in [
        "/api/secrets/status",
        "/api/secrets/unlock",
        "/api/secrets/items/opaque/value",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "neighbor_capability_denied");
        assert_eq!(body["reason"], "neighbor_capability_denied");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

async fn neighbor_save_matrix(case: &str) {
    use axum::{body::Body, http::Request, Router};
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(true).await;
    let router = crate::memory_router::memory_routes(Router::new())
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    codeterm_service_core::mem_ops::neighbor_save::matrix::run(
        case,
        &fixture,
        &concierge,
        |method, body| {
            let router = router.clone();
            let token = token.clone();
            async move {
                let token = if method == "owner" {
                    "owner-secret".to_string()
                } else {
                    token
                };
                let path = if method == "mem.save" {
                    "/api/mem/items"
                } else {
                    "/api/mem/neighbor-save"
                };
                let response = router
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(path)
                            .header("authorization", format!("Bearer {token}"))
                            .header("content-type", "application/json")
                            .body(Body::from(body.to_string()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap();
                let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                if status.is_success() {
                    Ok(value)
                } else {
                    Err((
                        status.as_u16(),
                        value["message"].as_str().unwrap().to_string(),
                    ))
                }
            }
        },
    )
    .await;
}

#[tokio::test]
async fn neighbor_save_saved_in_granted_area() {
    neighbor_save_matrix("saved_in_granted_area").await;
}

#[tokio::test]
async fn neighbor_save_area_b_refused() {
    neighbor_save_matrix("area_b_refused").await;
}

#[tokio::test]
async fn neighbor_save_empty_write_list() {
    neighbor_save_matrix("empty_write_list").await;
}

#[tokio::test]
async fn neighbor_save_mem_save_denied() {
    neighbor_save_matrix("mem_save_denied").await;
}

#[tokio::test]
async fn neighbor_save_forged_author_ignored() {
    neighbor_save_matrix("forged_author_ignored").await;
}

#[tokio::test]
async fn neighbor_save_rule_refused() {
    neighbor_save_matrix("rule_refused").await;
}

#[tokio::test]
async fn neighbor_save_preference_refused() {
    neighbor_save_matrix("preference_refused").await;
}

#[tokio::test]
async fn neighbor_save_skill_refused() {
    neighbor_save_matrix("skill_refused").await;
}

#[tokio::test]
async fn neighbor_save_script_refused() {
    neighbor_save_matrix("script_refused").await;
}

#[tokio::test]
async fn neighbor_save_forced_fields() {
    neighbor_save_matrix("forced_fields").await;
}

#[tokio::test]
async fn neighbor_save_credentials_title() {
    neighbor_save_matrix("credentials_title").await;
}

#[tokio::test]
async fn neighbor_save_credentials_body() {
    neighbor_save_matrix("credentials_body").await;
}

#[tokio::test]
async fn neighbor_save_credentials_summary() {
    neighbor_save_matrix("credentials_summary").await;
}

#[tokio::test]
async fn neighbor_save_credentials_tags() {
    neighbor_save_matrix("credentials_tags").await;
}

#[tokio::test]
async fn neighbor_save_over_cap_body() {
    neighbor_save_matrix("over_cap_body").await;
}

#[tokio::test]
async fn neighbor_save_over_cap_title() {
    neighbor_save_matrix("over_cap_title").await;
}

#[tokio::test]
async fn neighbor_save_over_cap_total() {
    neighbor_save_matrix("over_cap_total").await;
}

#[tokio::test]
async fn neighbor_save_revoked_area() {
    neighbor_save_matrix("revoked_area").await;
}

#[tokio::test]
async fn neighbor_save_paused_friend() {
    neighbor_save_matrix("paused_friend").await;
}

#[tokio::test]
async fn neighbor_save_owner_refused() {
    neighbor_save_matrix("owner_refused").await;
}

#[tokio::test]
async fn neighbor_save_never_extracts() {
    neighbor_save_matrix("never_extracts").await;
}

#[tokio::test]
async fn neighbor_save_missing_friend_author() {
    neighbor_save_matrix("missing_friend_author").await;
}

#[tokio::test]
async fn neighbor_save_replace_inside_grant() {
    neighbor_save_matrix("replace_inside_grant").await;
}

#[tokio::test]
async fn neighbor_save_replace_outside_grant() {
    neighbor_save_matrix("replace_outside_grant").await;
}

#[tokio::test]
async fn neighbor_save_replace_pinned() {
    neighbor_save_matrix("replace_pinned").await;
}
