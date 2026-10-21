use super::*;
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
