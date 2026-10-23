use super::*;

fn id() -> codeterm_account::InstallationId {
    codeterm_account::InstallationId::parse("fixture-installation".into()).unwrap()
}

#[test]
fn release_billing_base_ignores_saved_endpoint() {
    use codeterm_account::firebase_endpoints::PRODUCTION;
    for endpoint in [
        "http://127.0.0.1:15001/demo-domios/us-central1/",
        "https://override.test/",
    ] {
        assert_eq!(base_for(false, PRODUCTION, endpoint), PRODUCTION.functions);
        assert_eq!(base_for(true, PRODUCTION, endpoint), endpoint);
    }
}

#[tokio::test]
async fn build_endpoint_missing_uses_local_free_and_honest_actions() {
    let local = Cloud::configured_from(None, None, id()).unwrap();
    let session = Session {
        identity: codeterm_account::Identity {
            uid: "user".into(),
            email: None,
            display_name: None,
            provider: "local".into(),
            photo_url: None,
        },
        id_token: codeterm_account::Secret::new("test-token"),
        expires_at: None,
    };
    assert_eq!(
        local.entitlements(&session).await.unwrap(),
        Reading::current(Entitlements::default())
    );
    assert!(local
        .portal_url(&session)
        .await
        .unwrap_err()
        .contains("unavailable"));
    assert!(Cloud::configured_from(None, Some("https://example.test/"), id()).is_ok());
    assert!(Cloud::configured_from(Some("https://override.test/"), Some("invalid"), id()).is_ok());
}

#[cfg(not(debug_assertions))]
#[test]
fn release_constructor_ignores_configured_endpoint() {
    for endpoint in [
        "http://127.0.0.1:15001/demo-domios/us-central1/",
        "https://override.test/",
    ] {
        assert_eq!(
            Cloud::new(endpoint, id()).unwrap().base.as_str(),
            codeterm_account::firebase_endpoints::PRODUCTION.functions
        );
    }
}

#[test]
fn dev_issuer_is_loopback_http_in_debug_only() {
    let url = |value: &str| url::Url::parse(value).unwrap();
    assert_eq!(
        dev_issuer(&url("http://127.0.0.1:15200/")),
        cfg!(debug_assertions)
    );
    for refused in [
        "http://localhost:15200/",
        "http://192.168.1.2:15200/",
        "http://issuer.test/",
    ] {
        assert!(!dev_issuer(&url(refused)));
    }
}

#[test]
fn only_an_already_subscribed_refusal_keeps_its_reason() {
    let refusal = |reason: serde_json::Value| {
        refusal_message(
            &serde_json::json!({"error": {"status": "FAILED_PRECONDITION", "message": "opaque", "details": reason}}),
        )
    };
    assert_eq!(
        refusal(serde_json::json!({"reason": "already_subscribed"})),
        crate::ALREADY_SUBSCRIBED
    );
    assert_eq!(
        refusal(serde_json::json!({"reason": "something_else"})),
        "Billing request refused"
    );
    assert_eq!(refusal(serde_json::Value::Null), "Billing request refused");
    assert_eq!(
        refusal_message(&serde_json::Value::Null),
        "Billing request refused"
    );
}
