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
