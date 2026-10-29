use super::*;

#[test]
fn view_call_body_cannot_supply_host_context_or_omit_the_private_lease() {
    let valid = serde_json::json!({"pluginId":"studio","viewFrame":"host-lease","method":"read","args":{"owner":"not-authority"}});
    assert!(serde_json::from_value::<ViewCallReq>(valid.clone()).is_ok());
    let mut forged = valid.clone();
    forged["context"] = serde_json::json!({"owner":"other-owner","actor":"agent"});
    assert!(serde_json::from_value::<ViewCallReq>(forged).is_err());
    let mut missing = valid;
    missing.as_object_mut().unwrap().remove("viewFrame");
    assert!(serde_json::from_value::<ViewCallReq>(missing).is_err());
}

#[tokio::test]
async fn actual_view_call_route_refuses_non_owner_and_invalid_registration() {
    use axum::{body::Body, http::Request, Extension, Router};
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    use tower::ServiceExt;
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());
    let router = Router::new()
        .route(
            "/api/plugins/view-call",
            crate::plugin_host_action_routes::owner_post(plugin_view_call),
        )
        .with_state(state);
    for caller in [
        None,
        Some(crate::auth::CallerIdentity::Tab(
            codeterm_service_core::service::tab_credentials::TabCaller {
                tab_id: "agent".into(),
                binding_nonce: "nonce".into(),
            },
        )),
        Some(crate::auth::CallerIdentity::ServicePush {
            service_id: "other".into(),
        }),
    ] {
        let mut app = router.clone();
        if let Some(caller) = caller {
            app = app.layer(Extension(caller));
        }
        let response = app
            .oneshot(
                Request::post("/api/plugins/view-call")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let app = router.layer(Extension(crate::auth::CallerIdentity::Local));
    let response = app
        .oneshot(
            Request::post("/api/plugins/view-call")
                .header("authorization", "Bearer owner-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"pluginId":"studio","viewFrame":"forged","method":"read"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn narrow_host_ports_are_also_local_owner_only_before_body_or_callback() {
    use axum::{body::Body, http::Request, Extension, Router};
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    use tower::ServiceExt;
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    let router = Router::new()
        .route(
            "/api/plugins/remote-task-call",
            crate::plugin_host_action_routes::owner_post(remote_task_call),
        )
        .route(
            "/api/plugins/recording-suggestion",
            crate::plugin_host_action_routes::owner_post(recording_suggestion),
        )
        .with_state(state);
    for path in [
        "/api/plugins/remote-task-call",
        "/api/plugins/recording-suggestion",
    ] {
        let response = router
            .clone()
            .layer(Extension(crate::auth::CallerIdentity::Tab(
                codeterm_service_core::service::tab_credentials::TabCaller {
                    tab_id: "agent".into(),
                    binding_nonce: "nonce".into(),
                },
            )))
            .oneshot(Request::post(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert!(serde_json::from_value::<RemoteTaskReq>(
        serde_json::json!({"tabId":"tab","method":"read"})
    )
    .is_err());
    assert!(serde_json::from_value::<RecordingReq>(
        serde_json::json!({"pluginId":"studio","method":"submit","args":{}})
    )
    .is_err());
}

#[tokio::test]
async fn private_view_disposal_uses_a_fixed_owner_only_bounded_body_route() {
    use axum::{body::Body, http::Request, Extension, Router};
    use tower::ServiceExt;
    let path = "/api/plugins/view-frame/revoke";
    let router: Router = Router::new().route(
        path,
        crate::plugin_host_action_routes::owner_post(crate::plugin_media_routes::revoke_view)
            .layer(axum::extract::DefaultBodyLimit::max(1024)),
    );
    let body = serde_json::json!({"pluginId":"studio", "viewFrame":"a".repeat(64)}).to_string();
    let response = router
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let owner = router.layer(Extension(crate::auth::CallerIdentity::Local));
    for (body, expected) in [
        (body, StatusCode::NO_CONTENT),
        (
            serde_json::json!({"pluginId":"studio","viewFrame":"a".repeat(64),"owner":"forged"})
                .to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("x".repeat(1025), StatusCode::PAYLOAD_TOO_LARGE),
    ] {
        let response = owner
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert!(!path.contains(&"a".repeat(64)));
    assert!(!include_str!("../../routes.rs").contains("/api/plugins/{id}/view-frame/{frame}"));
}
