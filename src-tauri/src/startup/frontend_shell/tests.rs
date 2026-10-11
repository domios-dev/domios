use super::*;

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
    let mut config = crate::config::schema::ApiConfig::default();
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
