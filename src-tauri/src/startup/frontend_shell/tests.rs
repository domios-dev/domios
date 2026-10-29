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
fn frontend_shell_contract_confines_navigation_and_remote_capabilities() {
    let origin = "http://127.0.0.1:7686";
    let unavailable = unavailable_url().unwrap();
    let fallback = fallback_url().unwrap();
    assert_eq!(external_link(&unavailable, Some(origin), None), None);
    assert_eq!(external_link(&fallback, Some(origin), None), None);
    assert!(navigation_allowed(
        &unavailable,
        Some(origin),
        None,
        &unavailable,
        &fallback
    ));
    assert!(navigation_allowed(
        &fallback,
        Some(origin),
        None,
        &unavailable,
        &fallback
    ));
    assert!(navigation_allowed(
        &Url::parse(&format!("{origin}/bundled/index.html")).unwrap(),
        Some(origin),
        None,
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
            None,
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
        None,
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
fn reconnect_surface_data_url_keeps_spaces_and_styles() {
    let url = super::unavailable_url().unwrap();
    assert!(!url.as_str().contains('+'));
    assert!(url.as_str().contains("body%20%7B%20margin%3A%200%3B"));
}

#[test]
fn start_failure_page_carries_its_reason_without_changing_the_permitted_document() {
    let origin = "http://127.0.0.1:7686";
    let unavailable = unavailable_url().unwrap();
    let fallback = fallback_url().unwrap();
    let reason = "opaque-cause 17% <b>&\"#";
    let failed = start_failure_url(Some(reason)).unwrap();
    assert!(navigation_allowed(
        &failed,
        Some(origin),
        None,
        &unavailable,
        &fallback
    ));
    let document = start_failure_url(None).unwrap();
    assert!(same_navigation_document(&failed, &document));
    assert!(same_navigation_document(
        &start_failure_url(Some("another-cause")).unwrap(),
        &document
    ));
    let fragment = failed.fragment().unwrap();
    assert!(fragment
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"%*-._".contains(&byte)));
    let query = format!("r={fragment}");
    let decoded = url::form_urlencoded::parse(query.as_bytes())
        .next()
        .unwrap()
        .1;
    assert_eq!(decoded, reason);
    assert_eq!(start_failure_url(None).unwrap().fragment(), None);
}

#[test]
fn external_link_decision_table() {
    let origin = Some("http://127.0.0.1:7686");
    for (target, expected) in [
        ("http://127.0.0.1:7686/chat?tab=fixture#message", None),
        ("HTTP://127.0.0.1:7686/index.html", None),
        (
            "https://example.com/path?query=fixture#fragment",
            Some("https://example.com/path?query=fixture#fragment"),
        ),
        ("http://example.com/", Some("http://example.com/")),
        (
            "mailto:owner@example.com?subject=fixture",
            Some("mailto:owner@example.com?subject=fixture"),
        ),
        ("HTTPS://EXAMPLE.COM/path", Some("https://example.com/path")),
        ("HTTP://EXAMPLE.COM/", Some("http://example.com/")),
        ("MAILTO:owner@example.com", Some("mailto:owner@example.com")),
        ("javascript:alert(1)", None),
        ("JAVASCRIPT:alert(1)", None),
        ("data:text/html,unavailable", None),
        ("DATA:text/html,unavailable", None),
        ("file:///tmp/index.html", None),
        ("FILE:///tmp/index.html", None),
        ("tauri://localhost/index.html", None),
        ("http://tauri.localhost/index.html", None),
        ("https://tauri.localhost/index.html", None),
        ("about:blank", None),
        ("ftp://example.com/file", None),
        ("https://user@example.com/", None),
        ("https://:password@example.com/", None),
        ("https://user:password@example.com/", None),
        ("http://user@127.0.0.1:7686/", None),
    ] {
        let url = Url::parse(target).unwrap();
        assert_eq!(
            external_link(&url, origin, None).as_ref().map(Url::as_str),
            expected,
            "{target}"
        );
    }
    let external = Url::parse("https://example.com/").unwrap();
    assert_eq!(external_link(&external, None, None), Some(external.clone()));
    assert_eq!(
        external_link(&external, Some("https://example.com"), None),
        None
    );
}

#[test]
fn frontend_shell_contract_own_api_navigation_never_opens_externally() {
    let unavailable = unavailable_url().unwrap();
    let fallback = fallback_url().unwrap();
    for api in ["http://127.0.0.1:7685", "http://127.0.0.1:17901/"] {
        for captured in [
            None,
            Some("http://127.0.0.1:1420/"),
            Some("http://localhost:7686"),
        ] {
            for path in ["/", "/bundled/index.html"] {
                let url = Url::parse(api).unwrap().join(path).unwrap();
                let prepared = with_root_generation(
                    &url,
                    ClientViewGeneration {
                        view: 2,
                        navigation: 7,
                    },
                );
                for target in [&url, &prepared, &fallback] {
                    assert!(navigation_allowed(
                        target,
                        captured,
                        Some(api),
                        &unavailable,
                        &fallback,
                    ));
                    assert_eq!(external_link(target, captured, Some(api)), None);
                }
            }
        }
    }
}

#[test]
fn frontend_shell_contract_canonicalizes_frontend_and_loopback_api_origins() {
    let unavailable = unavailable_url().unwrap();
    let fallback = fallback_url().unwrap();
    for (target, frontend, api) in [
        (
            "http://127.0.0.1:7685/",
            None,
            Some("HTTP://127.0.0.1:7685/bundled/index.html"),
        ),
        (
            "http://localhost:7685/",
            None,
            Some("HTTP://LOCALHOST:7685/"),
        ),
        ("http://[::1]:7685/", None, Some("http://[::1]:7685/")),
        ("http://127.0.0.1/", None, Some("http://127.0.0.1:80/")),
        (
            "http://127.0.0.1:1420/",
            Some("HTTP://127.0.0.1:1420/index.html"),
            None,
        ),
    ] {
        let target = Url::parse(target).unwrap();
        assert!(navigation_allowed(
            &target,
            frontend,
            api,
            &unavailable,
            &fallback
        ));
        assert_eq!(external_link(&target, frontend, api), None);
    }
}

#[test]
fn frontend_shell_contract_keeps_genuine_external_links_and_rejects_credentials() {
    let api = Some("http://127.0.0.1:7685");
    let unavailable = unavailable_url().unwrap();
    let fallback = fallback_url().unwrap();
    for raw in [
        "https://example.com/path?query=fixture#fragment",
        "mailto:owner@example.com",
        "http://127.0.0.1:4180/",
        "http://127.0.0.2:7685/",
        "http://127.23.45.67:7685/",
        "http://localhost:7685/",
        "http://[::1]:7685/",
        "https://localhost:7685/",
        "http://localhost.example:7685/",
    ] {
        let target = Url::parse(raw).unwrap();
        assert!(!navigation_allowed(
            &target,
            None,
            api,
            &unavailable,
            &fallback
        ));
        assert_eq!(external_link(&target, None, api), Some(target));
    }
    for raw in [
        "http://user@localhost:7685/",
        "http://user:pass@127.0.0.1:7685/",
    ] {
        let target = Url::parse(raw).unwrap();
        assert!(!navigation_allowed(
            &target,
            None,
            api,
            &unavailable,
            &fallback
        ));
        assert_eq!(external_link(&target, None, api), None);
    }
}

#[test]
fn frontend_shell_contract_first_navigation_waits_for_starting_document_completion() {
    use tauri::webview::PageLoadEvent::{Finished, Started};
    let starting = starting_url().unwrap();
    assert!(!starting_document_finished(Started, &starting));
    assert!(starting_document_finished(Finished, &starting));
    for other in [unavailable_url().unwrap(), fallback_url().unwrap(), start_failure_url(None).unwrap()] {
        assert!(!starting_document_finished(Finished, &other));
    }
}
