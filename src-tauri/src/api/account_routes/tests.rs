use super::*;

#[test]
fn account_routes_admission_refuses_nonlocal_callers() {
    assert_eq!(admit(None).unwrap_err().0, StatusCode::FORBIDDEN);
    assert!(admit(Some(&Extension(CallerIdentity::Local))).is_ok());
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
async fn account_routes_local_journey_persists_and_deletes_credential() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::empty(),
    );
    *fixture.service.secrets.write().await =
        std::sync::Arc::new(crate::secrets::mock::MockStore::new());
    let state = crate::api::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    {
        use tauri::Manager;
        assert!(fixture._app.manage(state.config.clone()));
    }
    let router = crate::api::routes::create_router(state);
    let store = fixture.service.secrets.read().await.clone();
    for (method, path, signed_in) in [
        ("GET", "/api/account/status", false),
        ("POST", "/api/account/login", true),
        ("GET", "/api/account/status", true),
        ("POST", "/api/account/logout", false),
        ("GET", "/api/account/status", false),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", "Bearer fixture-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value["identity"].is_object(), signed_in);
        if signed_in {
            assert_eq!(value["identity"]["uid"], "local-dev-user");
        }
        assert_eq!(
            store.has("domios-account-local-refresh").await.unwrap(),
            signed_in
        );
        assert!(value.get("id_token").is_none());
    }
}

#[test]
fn account_plan_routes_payload_and_local_admission() {
    assert!(serde_json::from_value::<Upgrade>(serde_json::json!({"plan":"pro"})).is_ok());
    assert!(
        serde_json::from_value::<Upgrade>(serde_json::json!({"plan":"pro","uid":"foreign"}))
            .is_err()
    );
    assert_eq!(admit(None).unwrap_err().0, StatusCode::FORBIDDEN);
    let snapshot = codeterm_billing::Snapshot::default();
    let value = serde_json::to_value(snapshot).unwrap();
    assert_eq!(value["entitlements"]["plan"], "free");
    assert_eq!(value["stale"], false);
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
async fn account_plan_routes_local_journey() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::empty(),
    );
    *fixture.service.secrets.write().await = Arc::new(crate::secrets::mock::MockStore::new());
    let provider = Arc::new(codeterm_billing::adapters::fake::Fake::default());
    *provider.value.lock() = Ok(codeterm_billing::Entitlements {
        plan: "pro".into(),
        current_period_end: 2000000000,
        ..Default::default()
    });
    assert!(fixture
        .service
        .billing
        .set(Arc::new(codeterm_billing::Billing::new(provider.clone())))
        .is_ok());
    let state = crate::api::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    {
        use tauri::Manager;
        assert!(fixture._app.manage(state.config.clone()));
    }
    let router = crate::api::routes::create_router(state);
    for (method, path, expected_plan, stale) in [
        ("GET", "/api/account/plan", "free", false),
        ("POST", "/api/account/login", "", false),
        ("GET", "/api/account/plan", "pro", false),
        ("POST", "/api/account/logout", "", false),
        ("GET", "/api/account/plan", "free", false),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", "Bearer fixture-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        if !expected_plan.is_empty() {
            assert_eq!(value["entitlements"]["plan"], expected_plan);
            assert_eq!(value["stale"], stale);
        }
    }
    assert!(!fixture
        .service
        .secrets
        .read()
        .await
        .has("domios-account-local-refresh")
        .await
        .unwrap());
}
