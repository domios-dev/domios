use super::*;

#[test]
fn transport_capabilities_contract_host_os_controls_installation() {
    assert!(pwsh_install_supported("windows"));
    assert!(!pwsh_install_supported("macos"));
    assert!(!pwsh_install_supported("linux"));
}

#[test]
fn transport_capabilities_contract_path_body_and_bounds() {
    let body: PathKindsBody =
        serde_json::from_value(serde_json::json!({"paths": ["a", "b"]})).unwrap();
    assert_eq!(body.paths, vec!["a", "b"]);
    assert!(validate_paths(&body.paths).is_ok());
    assert!(serde_json::from_value::<PathKindsBody>(
        serde_json::json!({"paths": [], "tab_id": "foreign"})
    )
    .is_err());
    assert_eq!(
        validate_paths(&vec!["a".into(); 129]).unwrap_err().0,
        StatusCode::BAD_REQUEST
    );
    assert!(validate_paths(&["a\0b".into()]).is_err());
}

#[test]
fn transport_capabilities_contract_diagnostic_body_and_errors() {
    let body: WebglBody =
        serde_json::from_value(serde_json::json!({"state": "context_lost", "detail": "device"}))
            .unwrap();
    assert!(validate_webgl(&body).is_ok());
    assert!(serde_json::from_value::<WebglBody>(
        serde_json::json!({"state": "loaded", "command": "run"})
    )
    .is_err());
    assert_eq!(
        validate_webgl(&WebglBody {
            state: "arbitrary".into(),
            detail: None
        })
        .unwrap_err()
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(validate_webgl(&WebglBody {
        state: "failed".into(),
        detail: Some("x".repeat(4097))
    })
    .is_err());
}

#[test]
fn transport_capabilities_contract_theme_identity_confines_host_filesystem() {
    for id in [
        "../outside",
        "a/b",
        "a\\b",
        "/absolute",
        "C:drive",
        ".",
        "..",
        "",
    ] {
        assert!(!crate::theme_routes::valid_theme_id(id), "{id}");
    }
    assert!(crate::theme_routes::valid_theme_id("my_theme-2"));
}

#[tokio::test]
async fn transport_capabilities_contract_temporary_bytes_are_persisted_and_retrievable() {
    let temporary = tempfile::tempdir().unwrap();
    let bytes = [0, 1, 2, 3, 255];
    let name = crate::web_routes::sanitize_filename("../../contract.bin");
    let path = crate::web_routes::store_upload(temporary.path(), &name, &bytes)
        .await
        .unwrap();
    assert!(path.starts_with(temporary.path()));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        codeterm_service_core::attachments::dropped_path_kind(&path),
        "file"
    );
}

#[test]
fn transport_capabilities_contract_grouped_model_usage_is_not_quota() {
    let usage = codeterm_service_core::agent::provider::ModelGroupUsage {
        group: "group-a".into(),
        messages: 3,
        input_tokens: 4,
        output_tokens: 5,
        ..Default::default()
    };
    let value = serde_json::to_value(vec![usage]).unwrap();
    assert_eq!(value[0]["group"], "group-a");
    assert_eq!(value[0]["messages"], 3);
    assert_eq!(value[0]["input_tokens"], 4);
    assert!(value[0].get("spend_limit_cents").is_none());
}

#[test]
fn transport_capabilities_contract_push_tokens_cannot_access_host_routes() {
    for (method, path) in [
        ("GET", "/api/agents/providers/drop-in/model-usage"),
        ("GET", "/api/system/default-cwd"),
        ("GET", "/api/system/shell-diagnostics"),
        ("POST", "/api/system/install-pwsh"),
        ("GET", "/api/themes/location"),
        ("POST", "/api/attachments/path-kinds"),
        ("POST", "/api/diagnostics/webgl"),
    ] {
        assert!(!crate::auth::is_service_push_allowed(method, path));
    }
}

#[test]
fn transport_capabilities_contract_notification_answer_changes_record_once() {
    use codeterm_notification_core::contract::{adopt_caller_actions, answer_actions};
    use codeterm_service_core::notification::answer::AnswerAdmission;
    use codeterm_service_core::notification::store::{
        Ask, NotificationProvenance, NotificationSeverity, NotificationStore,
    };
    let mut store = NotificationStore::new(4);
    let actions = adopt_caller_actions(answer_actions(&["Yes".into(), "No".into()]).unwrap());
    let id = store.add_ext(
        "Question".into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: Some("asking-tab".into()),
            ..Default::default()
        },
        Ask::offering(Some(actions)),
        None,
        None,
    );
    assert_eq!(
        store.record_answer(&id, "Yes"),
        AnswerAdmission::Recorded("Yes".into())
    );
    assert_eq!(
        store.record_answer(&id, "No"),
        AnswerAdmission::AlreadyAnswered("Yes".into())
    );
    assert_eq!(
        store.record_answer(&id, "foreign"),
        AnswerAdmission::NotOffered
    );
}

mod http {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        middleware,
    };
    use codeterm_host_runtime::test_fixture::HostFixture as ServiceFixture;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn state(fixture: &ServiceFixture) -> AppState {
        let mut config = codeterm_service_core::config::schema::AppConfig::default();
        config.api.token = Some("capabilities-owner".into());
        let mut mesh =
            codeterm_service_core::mesh::MeshManager::new("capabilities-contract".into(), 0);
        mesh.disk_persist = false;
        AppState {
            app: fixture.app.handle().clone(),
            svc: Arc::new(fixture.service.clone()),
            cron: Arc::new(parking_lot::Mutex::new(
                codeterm_service_core::cron::store::CronStore::new_with_path(
                    fixture.temp.path().join("crons.json"),
                ),
            )),
            cron_wake: Arc::new(std::sync::Condvar::new()),
            api_port: 0,
            update_state: Arc::new(parking_lot::Mutex::new(
                codeterm_commands::update::UpdateState {
                    install_method: codeterm_commands::update::InstallMethod::Unknown,
                    last_check: None,
                    last_check_time: None,
                    channel: codeterm_commands::update::Channel::default(),
                    phase: codeterm_commands::update::UpdatePhase::Idle,
                    last_error: None,
                    phase_changed_at: None,
                    downloaded_bytes: 0,
                    download_total_bytes: None,
                },
            )),
            config: Arc::new(parking_lot::Mutex::new(config)),
            mesh: Arc::new(parking_lot::Mutex::new(mesh)),
            keepawake: Arc::new(parking_lot::Mutex::new(None)),
            resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
            rest_api: Arc::new(codeterm_service_core::startup::RestApiControl::new()),
            test_clock: None,
            events: fixture.service.events.clone(),
            web_dist: None,
        }
    }

    async fn request(
        app: &Router,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: serde_json::Value,
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        app.clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn provider_settings_contract_owner_routes() {
        let fixture = ServiceFixture::new(
            codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
        );
        fixture.service.agent_registry.register_provider(Box::new(
            crate::theme_routes::tests::SettingsProbe {
                path: fixture.temp.path().join("settings-result"),
            },
        ));
        let result_path = fixture.temp.path().join("settings-result");
        std::fs::write(result_path.with_extension("backup"), b"original").unwrap();
        let state = state(&fixture);
        let app = Router::new()
            .route(
                "/api/setup/rules",
                get(crate::theme_routes::check_rules_api),
            )
            .route(
                "/api/setup/rules/install",
                post(crate::theme_routes::install_rules_api),
            )
            .route(
                "/api/setup/hooks/restore",
                post(crate::theme_routes::restore_hooks_api),
            );
        let direct = app.clone().with_state(state.clone());
        let app = app
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(state);
        for (method, path) in [
            ("GET", "/api/setup/rules"),
            ("POST", "/api/setup/rules/install"),
            ("POST", "/api/setup/hooks/restore"),
        ] {
            assert_eq!(
                request(
                    &direct,
                    method,
                    &format!("{path}?provider=fixture"),
                    None,
                    serde_json::Value::Null
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                request(
                    &app,
                    method,
                    &format!("{path}?provider=fixture"),
                    None,
                    serde_json::Value::Null
                )
                .await
                .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                request(
                    &app,
                    method,
                    &format!("{path}?provider=unknown"),
                    Some("capabilities-owner"),
                    serde_json::Value::Null
                )
                .await
                .status(),
                StatusCode::NOT_FOUND
            );
            let response = request(
                &app,
                method,
                &format!("{path}?provider=fixture"),
                Some("capabilities-owner"),
                serde_json::Value::Null,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            if method == "GET" {
                assert_eq!(
                    body,
                    serde_json::json!({"installed": false, "update_available": false})
                );
            } else {
                assert_eq!(body, serde_json::json!({"ok": true}));
                assert_eq!(
                    std::fs::read(&result_path).unwrap(),
                    if path.ends_with("restore") {
                        b"original".as_slice()
                    } else {
                        b"rule".as_slice()
                    }
                );
            }
        }
        std::fs::remove_file(result_path.with_extension("backup")).unwrap();
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/setup/hooks/restore?provider=fixture",
                Some("capabilities-owner"),
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(std::fs::read(result_path).unwrap(), b"original");
    }

    #[tokio::test]
    async fn transport_capabilities_contract_new_routes_auth_results_and_errors() {
        let fixture = ServiceFixture::new(
            codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
        );
        let state = state(&fixture);
        let app = router()
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(state);
        let routes = [
            (
                "GET",
                "/api/agents/providers/unknown/model-usage",
                serde_json::json!(null),
            ),
            ("GET", "/api/system/default-cwd", serde_json::json!(null)),
            (
                "GET",
                "/api/system/shell-diagnostics",
                serde_json::json!(null),
            ),
            ("POST", "/api/system/install-pwsh", serde_json::json!(null)),
            ("GET", "/api/themes/location", serde_json::json!(null)),
            (
                "POST",
                "/api/attachments/path-kinds",
                serde_json::json!({"paths": []}),
            ),
            (
                "POST",
                "/api/diagnostics/webgl",
                serde_json::json!({"state": "loaded"}),
            ),
        ];
        for (method, path, body) in &routes {
            for token in [None, Some("invalid")] {
                assert_eq!(
                    request(&app, method, path, token, body.clone())
                        .await
                        .status(),
                    StatusCode::UNAUTHORIZED,
                    "{path}"
                );
            }
        }
        for (method, path, body) in routes
            .into_iter()
            .filter(|(_, path, _)| *path != "/api/system/install-pwsh")
        {
            let response = request(&app, method, path, Some("capabilities-owner"), body).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
            let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            match path {
                "/api/agents/providers/unknown/model-usage" | "/api/attachments/path-kinds" => {
                    assert_eq!(result, serde_json::json!([]))
                }
                "/api/system/default-cwd" => assert_eq!(
                    result,
                    serde_json::json!(codeterm_commands::commands::get_default_cwd().unwrap())
                ),
                "/api/system/shell-diagnostics" => {
                    assert_eq!(result["platform"], std::env::consts::OS)
                }
                "/api/themes/location" => {
                    assert!(result.as_str().is_some_and(|path| !path.is_empty()))
                }
                "/api/diagnostics/webgl" => assert_eq!(result, serde_json::Value::Null),
                _ => unreachable!(),
            }
        }
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/diagnostics/webgl",
                Some("capabilities-owner"),
                serde_json::json!({"state": "invalid"})
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/attachments/path-kinds",
                Some("capabilities-owner"),
                serde_json::json!({"paths": vec!["a"; 129]})
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/system/install-pwsh",
                Some("capabilities-owner"),
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
    #[tokio::test]
    async fn transport_capabilities_contract_integration_refusal_and_config_patch() {
        let fixture = ServiceFixture::new(
            codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
        );
        let state = state(&fixture);
        let app = Router::new()
            .route(
                "/api/setup/hooks/install",
                post(crate::theme_routes::install_hooks_api),
            )
            .route(
                "/api/setup/hooks/uninstall",
                post(crate::theme_routes::uninstall_hooks_api),
            )
            .route(
                "/api/config",
                get(crate::config_routes::get_config_api)
                    .patch(crate::config_routes::patch_config_api),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(state);
        for path in [
            "/api/setup/hooks/install?provider=unknown",
            "/api/setup/hooks/uninstall?provider=unknown",
        ] {
            assert_eq!(
                request(&app, "POST", path, None, serde_json::Value::Null)
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                request(
                    &app,
                    "POST",
                    path,
                    Some("capabilities-owner"),
                    serde_json::Value::Null
                )
                .await
                .status(),
                StatusCode::NOT_FOUND
            );
        }
        let patch = serde_json::json!({"key": "notifications.bell", "value": true});
        assert_eq!(
            request(&app, "PATCH", "/api/config", None, patch.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "PATCH",
                "/api/config",
                Some("capabilities-owner"),
                patch.clone()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/config",
                Some("capabilities-owner"),
                patch
            )
            .await
            .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}
