use super::*;
#[test]
fn paste_requires_whole_callback_url() {
    assert!(matches!(
        Callback::parse_paste(&"a".repeat(43)),
        Err(PasteCallbackError::CallbackUrlRequired)
    ));
    for suffix in ["&code=duplicate", "&state=duplicate", "#fragment"] {
        let input = format!(
            "http://127.0.0.1:8765/callback?code={}&state=0123456789abcdef{suffix}",
            "a".repeat(43)
        );
        assert!(Callback::parse_paste(&input).is_err());
    }
}

#[test]
fn pasted_url_keeps_state_verifier_and_single_use() {
    let mut flow = LoginFlow::from_parts(
        "0123456789abcdef".into(),
        Secret::new("original-verifier"),
        "http://127.0.0.1:8765/callback".into(),
    );
    let wrong = format!(
        "http://127.0.0.1:8765/callback?code={}&state=wrong-state-0123456789",
        "a".repeat(43)
    );
    assert!(matches!(
        flow.complete(Callback::parse_paste(&wrong).unwrap()),
        Err(Error::WrongState)
    ));
    let correct = format!(
        "http://127.0.0.1:8765/callback?code={}&state=0123456789abcdef",
        "a".repeat(43)
    );
    let request = flow
        .complete(Callback::parse_paste(&correct).unwrap())
        .unwrap();
    assert_eq!(request.verifier.expose(), "original-verifier");
    assert_eq!(request.state, "0123456789abcdef");
    assert_eq!(request.redirect, "http://127.0.0.1:8765/callback");
    assert!(matches!(
        flow.complete(Callback::parse_paste(&correct).unwrap()),
        Err(Error::Consumed)
    ));
}
#[test]
fn signin_url_preserves_pkce_and_adds_only_typed_hints() {
    for (provider, expected) in [
        (LoginProvider::Google, "google"),
        (LoginProvider::Facebook, "facebook"),
        (LoginProvider::Email, "email"),
    ] {
        for (mode, expected_mode) in [(LoginMode::Signin, "signin"), (LoginMode::Signup, "signup")]
        {
            let flow = LoginFlow::from_parts(
                "state-fixture".into(),
                Secret::new("verifier-fixture"),
                "domios://auth/callback".into(),
            )
            .with_hint(LoginHint {
                provider: Some(provider),
                mode: Some(mode),
            });
            let url = flow.signin_url("https://example.test/signin/").unwrap();
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(pairs.len(), 5);
            assert_eq!(pairs["provider"], expected);
            assert_eq!(pairs["mode"], expected_mode);
            assert_eq!(pairs["state"], "state-fixture");
            assert_eq!(
                pairs["code_challenge"],
                LoginFlow::challenge("verifier-fixture")
            );
            assert_eq!(pairs["redirect_uri"], "domios://auth/callback");
            assert!(!url.as_str().contains("verifier-fixture"));
        }
    }
    assert!(serde_json::from_value::<LoginHint>(serde_json::json!({"provider":"apple"})).is_err());
    assert!(serde_json::from_value::<LoginHint>(serde_json::json!({"mode":"unknown"})).is_err());
}
