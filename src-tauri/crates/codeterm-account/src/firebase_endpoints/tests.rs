use super::*;

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
