use super::*;

#[test]
fn frontend_shell_contract_reopen_wakes_the_existing_supervisor_without_listener_restart() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    initialize(app.handle()).unwrap();
    let supervisor = app.state::<ShellSupervisor>();
    let mut windows = supervisor.windows.subscribe();
    assert!(!windows.borrow_and_update().available);
    window_created(app.handle());
    assert!(windows.has_changed().unwrap());
    let first = *windows.borrow_and_update();
    assert!(first.available);
    window_destroying(app.handle());
    assert!(windows.has_changed().unwrap());
    assert!(!windows.borrow_and_update().available);
    assert!(supervisor.navigation.load(Ordering::Acquire) > 0);
    window_created(app.handle());
    assert!(windows.has_changed().unwrap());
    let reopened = *windows.borrow_and_update();
    assert!(reopened.available);
    assert!(reopened.generation > first.generation);
}

#[test]
fn frontend_shell_contract_requires_health_proven_bind_acknowledgement() {
    let mut status = RestApiStatus {
        generation: 1,
        phase: RestApiPhase::Starting,
        diagnostic: None,
        message: None,
    };
    let address = Some("127.0.0.1:7686".parse().unwrap());
    assert_eq!(ready_origin(&status, address).unwrap(), None);
    status.phase = RestApiPhase::Failed;
    assert_eq!(ready_origin(&status, address).unwrap(), None);
    status.phase = RestApiPhase::Listening;
    assert!(ready_origin(&status, None).is_err());
    assert_eq!(
        ready_origin(&status, address).unwrap(),
        Some("http://127.0.0.1:7686".into())
    );
}

#[test]
fn frontend_shell_contract_uses_only_loopback_http_origins() {
    for (address, expected) in [
        ("127.0.0.1:7686", "http://127.0.0.1:7686"),
        ("0.0.0.0:7686", "http://127.0.0.1:7686"),
        ("[::]:7686", "http://[::1]:7686"),
        ("[::1]:7686", "http://[::1]:7686"),
    ] {
        assert_eq!(loopback_origin(address.parse().unwrap()).unwrap(), expected);
    }
    assert!(loopback_origin("192.168.1.2:7686".parse().unwrap()).is_err());
}

#[test]
fn frontend_shell_contract_refuses_missing_credentials_or_disabled_listener() {
    let mut config = codeterm_service_core::config::schema::ApiConfig::default();
    config.port = 18886;
    config.enabled = true;
    config.token = None;
    assert!(bootstrap(&config).is_err());
    config.token = Some("".into());
    assert!(bootstrap(&config).is_err());
    config.token = Some("fixture-token".into());
    assert_eq!(bootstrap(&config).unwrap().token, "fixture-token");
    config.enabled = false;
    assert!(bootstrap(&config).is_err());
}

#[test]
fn frontend_shell_contract_confines_navigation_and_remote_capabilities() {
    let origin = "http://127.0.0.1:7686";
    let unavailable = unavailable_url().unwrap();
    let fallback = fallback_url().unwrap();
    assert_eq!(external_link(&unavailable, Some(origin)), None);
    assert_eq!(external_link(&fallback, Some(origin)), None);
    assert!(navigation_allowed(
        &unavailable,
        Some(origin),
        &unavailable,
        &fallback
    ));
    assert!(navigation_allowed(
        &fallback,
        Some(origin),
        &unavailable,
        &fallback
    ));
    assert!(navigation_allowed(
        &Url::parse(&format!("{origin}/bundled/index.html")).unwrap(),
        Some(origin),
        &unavailable,
        &fallback
    ));
    for target in [
        "http://127.0.0.1:7685",
        "https://foreign.example",
        "data:text/html,other",
        "http://user@127.0.0.1:7686",
    ] {
        assert!(!navigation_allowed(
            &Url::parse(target).unwrap(),
            Some(origin),
            &unavailable,
            &fallback
        ));
    }
    let capability: serde_json::Value =
        serde_json::from_str(&daemon_capability(origin).unwrap()).unwrap();
    assert_eq!(capability["local"], false);
    assert_eq!(
        capability["remote"]["urls"],
        serde_json::json!(["http://127.0.0.1:7686/*"])
    );
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
}

#[test]
fn frontend_shell_contract_binds_root_acknowledgement_to_document_generation() {
    let url = Url::parse("http://127.0.0.1:7686/index.html?keep=1&__codeterm_client_view=old#tab")
        .unwrap();
    let generation = ClientViewGeneration {
        view: 7,
        navigation: 11,
    };
    let prepared = with_root_generation(&url, generation);

    assert_eq!(root_generation_from_url(&prepared), Some(generation));
    assert_eq!(
        prepared
            .query_pairs()
            .filter(|(name, _)| name == CLIENT_VIEW_QUERY)
            .count(),
        1
    );
    assert!(navigation_allowed(
        &prepared,
        Some("http://127.0.0.1:7686"),
        &unavailable_url().unwrap(),
        &fallback_url().unwrap()
    ));
}

#[test]
fn frontend_shell_contract_keeps_page_initiated_documents_and_never_loops_the_fallback() {
    let daemon = Url::parse("http://127.0.0.1:7686/bundled/index.html").unwrap();
    let fallback = fallback_url().unwrap();
    let generation = ClientViewGeneration {
        view: 1,
        navigation: 17,
    };

    assert_eq!(
        unrendered_root_action(&daemon, &fallback, false),
        UnrenderedRoot::KeepDocument
    );
    assert_eq!(
        unrendered_root_action(&with_root_generation(&daemon, generation), &fallback, true),
        UnrenderedRoot::PackagedFallback
    );
    assert_eq!(
        unrendered_root_action(
            &with_root_generation(&fallback, generation),
            &fallback,
            true
        ),
        UnrenderedRoot::Unavailable
    );
}

#[test]
fn frontend_shell_contract_navigation_diagnostics_omit_query_secrets_and_recovery_html() {
    let secret = "opaque-fixture";
    let url = Url::parse(&format!(
        "http://127.0.0.1:1463/index.html?token={secret}#{secret}"
    ))
    .unwrap();
    assert_eq!(
        navigation_identity(&url),
        "http://127.0.0.1:1463/index.html"
    );
    assert!(!navigation_identity(&unavailable_url().unwrap()).contains("html"));
}

#[test]
fn frontend_shell_contract_limits_native_permissions_to_the_exact_frontend_origin() {
    for origin in ["http://127.0.0.1:7690", "http://127.0.0.1:1463"] {
        let capability: serde_json::Value =
            serde_json::from_str(&daemon_capability(origin).unwrap()).unwrap();
        assert_eq!(capability["local"], false);
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        assert_eq!(
            capability["remote"]["urls"],
            serde_json::json!([format!("{origin}/*")])
        );
        let permissions = capability["permissions"].as_array().unwrap();
        assert!(permissions.iter().all(|permission| {
            let permission = permission.as_str().unwrap();
            !permission.ends_with(":default")
                && !permission.starts_with("shell:")
                && !permission.starts_with("notification:")
                && !permission.starts_with("updater:")
        }));
    }
}

#[test]
fn frontend_shell_contract_cors_origins_match_declared_navigation_origins() {
    let mut config = codeterm_service_core::config::schema::ApiConfig::default();
    config.port = 7690;
    let dev = Url::parse("http://127.0.0.1:1463/").unwrap();
    let origins = declared_frontend_origins(&config, Some(&dev));
    assert!(origins.contains(&fallback_origin().to_string()));
    assert!(origins.contains(&"http://127.0.0.1:7690".into()));
    assert_eq!(
        origins.contains(&"http://127.0.0.1:1463".into()),
        cfg!(debug_assertions)
    );
    assert!(!origins.contains(&"http://localhost:1463".into()));
    assert!(!origins.contains(&"https://tauri.localhost".into()));
}

#[test]
fn reconnect_surface_data_url_keeps_spaces_and_styles() {
    let url = super::unavailable_url().unwrap();
    assert!(!url.as_str().contains('+'));
    assert!(url.as_str().contains("body%20%7B%20margin%3A%200%3B"));
}
