use super::{validate_after_update_enabled, validate_initiating_tab};

async fn assert_container_api_refusal(method: axum::http::Method, path: &str) {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        Extension,
    };
    use tower::ServiceExt;

    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let update = state.update_state.clone();
    let cached = codeterm_update::update_state::UpdateInfo {
        availability: codeterm_plugins::AvailabilityItem::release(
            "app",
            codeterm_plugins::UpdateKind::App,
            "1.0.0",
            "2.0.0",
            true,
        ),
        current_version: "1.0.0".into(),
        latest_version: "2.0.0".into(),
        update_available: true,
        install_method: codeterm_update::InstallMethod::Unknown,
        update_command: None,
        download_url: None,
        release_url: String::new(),
        release_notes: None,
        published_at: None,
        check_path: None,
    };
    let cached_json = serde_json::to_value(&cached).unwrap();
    {
        let mut update = update.lock();
        update.last_check = Some(cached);
        update.last_check_time = Some(std::time::Instant::now());
    }
    let root = fixture.temp.path().join("container-bundle");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join(codeterm_paths::container_install::MARKER), b"").unwrap();
    let app = crate::routes::create_router(state).layer(Extension(super::InstalledExecutable(
        root.join("domios-server/domios-server"),
    )));
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "container_managed_update");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("Pull a newer image tag"));
    let update = update.lock();
    assert_eq!(update.phase, codeterm_update::UpdatePhase::Idle);
    assert!(update.last_error.is_none());
    assert_eq!(
        serde_json::to_value(update.last_check.as_ref().unwrap()).unwrap(),
        cached_json
    );
}

#[tokio::test]
async fn container_check_refuses_before_cached_offer_or_network() {
    assert_container_api_refusal(axum::http::Method::GET, "/api/update/check").await;
}

#[tokio::test]
async fn container_availability_refuses_before_discovery() {
    assert_container_api_refusal(axum::http::Method::GET, "/api/update/availability").await;
}

#[tokio::test]
async fn container_install_refuses_before_effects() {
    assert_container_api_refusal(axum::http::Method::POST, "/api/update/install?method=auto").await;
}

#[test]
fn after_update_request_is_refused_when_resume_is_disabled() {
    assert_eq!(validate_after_update_enabled(true, false), Err("disabled"));
    assert!(validate_after_update_enabled(false, false).is_ok());
    assert!(validate_after_update_enabled(true, true).is_ok());
}

#[test]
fn auto_update_initiating_tab_must_match_authenticated_caller() {
    assert_eq!(
        validate_initiating_tab(Some("caller"), Some("other")),
        Err("not_caller_tab")
    );
    assert!(validate_initiating_tab(Some("caller"), Some("caller")).is_ok());
    assert!(validate_initiating_tab(None, None).is_ok());
}

#[tokio::test]
async fn auto_install_without_native_host_returns_actionable_typed_refusal() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    assert!(codeterm_commands::application_host::require(&state.svc).is_err());
    let update = state.update_state.clone();
    let app = crate::routes::create_router(state);
    let response = app
        .oneshot(
            Request::post("/api/update/install?method=auto")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "native_capability_unavailable");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("desktop client"));
    assert!(message.contains("domios update install --from-file <installer-path>"));
    assert_eq!(update.lock().phase, codeterm_update::UpdatePhase::Idle);
    assert!(update.lock().last_error.is_none());
}

#[tokio::test]
async fn auto_install_with_native_host_reaches_the_registered_updater() {
    use axum::{body::Body, http::Request};
    use codeterm_update::{
        application_host::ApplicationHost,
        apply::{ApplyOptions, ApplyResult},
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    struct Host(parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<ApplyOptions>>>);
    #[codeterm_update::application_host::async_trait]
    impl ApplicationHost for Host {
        fn restart(&self) -> Result<(), String> {
            Err("Unexpected restart".into())
        }
        fn exit(&self, _: i32) -> Result<(), String> {
            Err("Unexpected exit".into())
        }
        async fn copy_image_to_clipboard(&self, _: String) -> Result<(), String> {
            Err("Unexpected clipboard".into())
        }
        async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String> {
            assert!(self.0.lock().take().unwrap().send(options).is_ok());
            Ok(ApplyResult::NoUpdate)
        }
    }
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let (sender, received) = tokio::sync::oneshot::channel();
    assert!(fixture
        .service
        .register_host_extension(Arc::new(
            codeterm_commands::application_host::ApplicationHostState(Arc::new(Host(
                parking_lot::Mutex::new(Some(sender))
            )))
        ))
        .is_ok());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let response = crate::routes::create_router(state)
        .oneshot(
            Request::post("/api/update/install?method=auto")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let options = tokio::time::timeout(std::time::Duration::from_secs(1), received)
        .await
        .unwrap()
        .unwrap();
    assert!(options.manifest_url.is_none());
}
