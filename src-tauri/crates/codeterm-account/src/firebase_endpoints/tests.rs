use super::*;

fn qa_fixture() -> qa::Qa {
    qa::Qa::new(
        Some("domios-signin-qa"),
        Some("qa-key"),
        Some("qa-app"),
        Some("prod-key"),
        Some("prod-app"),
    )
    .unwrap()
}

#[test]
fn debug_qa_builds_one_isolated_endpoint_set() {
    assert_eq!(qa::selected(true, Some("qa"), None), Ok(true));
    let qa = Box::leak(Box::new(qa_fixture()));
    let endpoints = qa.endpoints();
    assert_eq!(qa.project, "domios-signin-qa");
    assert_eq!(qa.api_key, "qa-key");
    assert_eq!(qa.app_id, "qa-app");
    assert_eq!(endpoints.identity_toolkit, PRODUCTION.identity_toolkit);
    assert_eq!(endpoints.secure_token, PRODUCTION.secure_token);
    assert_eq!(
        endpoints.functions,
        "https://us-central1-domios-signin-qa.cloudfunctions.net/"
    );
    assert_eq!(
        endpoints.redeem,
        format!("{}redeemDesktopLoginCode", endpoints.functions)
    );
    assert_eq!(endpoints.signin, "https://domios-signin-qa.web.app/signin/");
    assert!(endpoints.qa && !endpoints.emulator);
    let mut config = crate::Config {
        provider: "firebase".into(),
        api_key: qa.api_key.into(),
        signin_url: endpoints.signin.into(),
        redeem_url: endpoints.redeem.into(),
        login_timeout_secs: 180,
    };
    assert!(qa.validate_account(&config).is_ok());
    config.redeem_url = PRODUCTION.redeem.into();
    assert!(qa.validate_account(&config).is_err());
    config.redeem_url = endpoints.redeem.into();
    config.api_key = "prod-key".into();
    assert!(qa.validate_account(&config).is_err());
}

#[test]
fn qa_missing_and_mixed_build_configuration_is_refused() {
    for (project, key, app) in [
        (None, Some("qa-key"), Some("qa-app")),
        (Some("domios-signin-qa"), None, Some("qa-app")),
        (Some("domios-signin-qa"), Some("qa-key"), Some(" ")),
        (Some("domios-d9357"), Some("qa-key"), Some("qa-app")),
        (Some("domios-signin-qa"), Some("prod-key"), Some("qa-app")),
        (Some("domios-signin-qa"), Some("qa-key"), Some("prod-app")),
    ] {
        assert!(qa::Qa::new(project, key, app, Some("prod-key"), Some("prod-app")).is_err());
    }
    assert!(qa::selected(true, Some("qa"), Some("1")).is_err());
    assert!(qa::selected(true, Some("production"), None).is_err());
}

#[test]
fn release_ignores_qa_target_and_debug_default_stays_production() {
    for target in [None, Some("qa"), Some("invalid")] {
        assert_eq!(qa::selected(false, target, Some("1")), Ok(false));
    }
    assert_eq!(qa::selected(true, None, None), Ok(false));
    assert_eq!(resolve(true, None), PRODUCTION);
}

#[test]
fn release_resolver_ignores_emulator_opt_in() {
    for value in [None, Some("1"), Some("true"), Some("0")] {
        assert_eq!(resolve(false, value), PRODUCTION);
    }
}

#[test]
fn release_ignores_saved_signin_and_redeem_overrides() {
    let mut config = crate::Config {
        provider: "firebase".into(),
        api_key: "fixture".into(),
        login_timeout_secs: 180,
        signin_url: EMULATOR.signin.into(),
        redeem_url: EMULATOR.redeem.into(),
    };
    apply_account_endpoints(&mut config, false, EMULATOR);
    assert_eq!(config.signin_url, PRODUCTION.signin);
    assert_eq!(config.redeem_url, PRODUCTION.redeem);
    apply_account_endpoints(&mut config, true, EMULATOR);
    assert_eq!(config.signin_url, EMULATOR.signin);
    assert_eq!(config.redeem_url, EMULATOR.redeem);
    assert_eq!(config.api_key, "FAKE-local-api-key");
}

#[test]
fn dev_requires_explicit_opt_in_and_targets_demo_project() {
    for value in [None, Some("true"), Some("0"), Some("")] {
        assert_eq!(resolve(true, value), PRODUCTION);
    }
    let local = resolve(true, Some("1"));
    assert_eq!(
        local.identity_toolkit,
        "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/"
    );
    assert_eq!(
        local.secure_token,
        "http://127.0.0.1:19099/securetoken.googleapis.com/v1/"
    );
    assert_eq!(
        local.functions,
        "http://127.0.0.1:15001/demo-domios/us-central1/"
    );
    assert_eq!(
        local.redeem,
        format!("{}redeemDesktopLoginCode", local.functions)
    );
    assert_eq!(local.signin, "http://127.0.0.1:15000/signin/");
}

#[test]
fn http_is_confined_to_opted_in_emulator_services() {
    for base in [
        EMULATOR.identity_toolkit,
        EMULATOR.secure_token,
        EMULATOR.functions,
        EMULATOR.signin,
    ] {
        let url = url::Url::parse(base).unwrap();
        assert!(matches_emulator_url(&url, resolve(true, Some("1"))));
        assert!(!matches_emulator_url(&url, resolve(false, Some("1"))));
        assert!(!matches_emulator_url(&url, resolve(true, None)));
    }
    for target in [
        "http://127.0.0.1:15001/domios-d9357/us-central1/getEntitlements",
        "http://127.0.0.1:15001/demo-domios/us-central1/../../other",
        "http://127.0.0.1:19099/other-api/v1/token",
        "http://127.0.0.1:15000/signin-evil/",
        "http://127.0.0.1:15000/",
        "http://127.0.0.1:15000.evil.invalid/signin/",
        "http://user@127.0.0.1:15000/signin/",
        "http://192.0.2.1:15000/signin/",
    ] {
        if let Ok(url) = url::Url::parse(target) {
            assert!(!matches_emulator_url(&url, EMULATOR), "{target}");
        }
    }
}

#[cfg(not(debug_assertions))]
#[test]
fn compiled_release_ignores_account_config_and_refuses_http() {
    let config = account_config(crate::Config {
        signin_url: EMULATOR.signin.into(),
        redeem_url: EMULATOR.redeem.into(),
        ..crate::Config::default()
    });
    assert_eq!(config.signin_url, PRODUCTION.signin);
    assert_eq!(config.redeem_url, PRODUCTION.redeem);
    assert_eq!(current(), PRODUCTION);
    assert!(!allows_url(&url::Url::parse(EMULATOR.redeem).unwrap()));
}
