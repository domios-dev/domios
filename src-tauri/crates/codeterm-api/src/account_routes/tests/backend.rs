use super::*;

#[test]
fn backend_projection_reports_actual_endpoint_kind_and_never_exposes_urls_or_secrets() {
    let production = codeterm_account::firebase_endpoints::PRODUCTION;
    for (qa, emulator, functions, target, project, kind) in [
        (
            false,
            false,
            production.functions,
            "production",
            Some("domios-d9357"),
            "production",
        ),
        (
            true,
            false,
            "https://us-central1-domios-signin-qa.cloudfunctions.net/",
            "qa",
            Some("domios-signin-qa"),
            "qa",
        ),
        (
            false,
            true,
            "http://127.0.0.1:15001/demo-domios/us-central1/",
            "emulator",
            Some("demo-domios"),
            "emulator",
        ),
        (
            true,
            false,
            production.functions,
            "qa",
            Some("domios-d9357"),
            "production",
        ),
        (
            true,
            false,
            "https://opaque:secret@example.invalid/?token=opaque",
            "qa",
            None,
            "unknown",
        ),
    ] {
        let view = backend_view(codeterm_account::firebase_endpoints::Endpoints {
            qa,
            emulator,
            functions,
            ..production
        });
        assert_eq!(
            view,
            serde_json::json!({"schema_version":1,"target":target,"project_id":project,"functions_endpoint_kind":kind})
        );
    }
}

#[tokio::test]
async fn backend_route_requires_local_owner_auth_and_reports_the_executing_endpoint_selection() {
    use tower::ServiceExt;
    let (mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let tab_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let router = crate::routes::create_router(state);
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(tab_token.as_str()), StatusCode::FORBIDDEN),
        (Some("preview-test-token"), StatusCode::OK),
    ] {
        let mut request = axum::http::Request::builder().uri("/api/account/backend");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            assert_eq!(
                response.headers()[axum::http::header::CACHE_CONTROL],
                "no-store"
            );
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                backend_view(codeterm_account::firebase_endpoints::current())
            );
        }
    }
    assert!(backend(None).await.is_err());
    assert!(backend(Some(Extension(CallerIdentity::ServicePush {
        service_id: "opaque".into()
    })))
    .await
    .is_err());
    assert!(backend(Some(Extension(CallerIdentity::Peer {
        instance_id: "opaque".into(),
        access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
        relay_protocol_version: 1,
    })))
    .await
    .is_err());
}
