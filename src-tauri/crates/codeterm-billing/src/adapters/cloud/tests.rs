use super::*;

fn id() -> codeterm_account::InstallationId {
    codeterm_account::InstallationId::parse("fixture-installation".into()).unwrap()
}

fn local_session() -> Session {
    Session {
        identity: codeterm_account::Identity {
            uid: "local-dev-user".into(),
            provider: "local".into(),
            email: None,
            display_name: None,
            photo_url: None,
        },
        id_token: codeterm_account::Secret::new("local-memory-session"),
        expires_at: None,
    }
}

fn request(provider: &Cloud, session: &Session, route: &str) -> reqwest::Request {
    provider
        .request(session, route, serde_json::json!({"installation_id":id()}))
        .unwrap()
        .build()
        .unwrap()
}

#[cfg(debug_assertions)]
#[test]
fn local_bearer_is_confined_to_debug_loopback_entitlements() {
    let session = local_session();
    let cloud = Cloud::new("http://127.0.0.1:15200/", id()).unwrap();
    let allowed = request(&cloud, &session, "getEntitlements");
    assert_eq!(
        allowed.headers()["authorization"],
        "Bearer domios-local-dev.uid=local-dev-user&installation_id=fixture-installation"
    );
    for base in [
        "https://127.0.0.1:15200/",
        "http://localhost:15200/",
        "http://192.168.1.2:15200/",
        "https://issuer.test/",
        codeterm_account::firebase_endpoints::PRODUCTION.functions,
    ] {
        let refused = Cloud::fixture(base, id());
        assert_eq!(
            request(&refused, &session, "getEntitlements").headers()["authorization"],
            "Bearer local-memory-session",
            "{base}"
        );
    }
    for route in [
        "createCheckoutSession",
        "createPortalSession",
        "https://issuer.test/getEntitlements",
    ] {
        assert_eq!(
            request(&cloud, &session, route).headers()["authorization"],
            "Bearer local-memory-session"
        );
    }
    for (provider, uid) in [("firebase", "local-dev-user"), ("local", "other-user")] {
        let mut other = session.clone();
        other.identity.provider = provider.into();
        other.identity.uid = uid.into();
        assert_eq!(
            request(&cloud, &other, "getEntitlements").headers()["authorization"],
            "Bearer local-memory-session"
        );
    }
}

#[cfg(not(debug_assertions))]
#[test]
fn release_local_session_has_no_dev_bearer() {
    let cloud = Cloud::new("http://127.0.0.1:15200/", id()).unwrap();
    let request = request(&cloud, &local_session(), "getEntitlements");
    assert_eq!(
        request.url().as_str(),
        format!(
            "{}getEntitlements",
            codeterm_account::firebase_endpoints::PRODUCTION.functions
        )
    );
    assert_eq!(
        request.headers()["authorization"],
        "Bearer local-memory-session"
    );
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
#[test]
fn qa_billing_requires_the_selected_project() {
    let endpoints = codeterm_account::firebase_endpoints::Endpoints {
        qa: true,
        functions: "https://us-central1-domios-signin-qa.cloudfunctions.net/",
        ..codeterm_account::firebase_endpoints::PRODUCTION
    };
    assert!(super::validate_qa_base(endpoints, None).is_ok());
    assert!(super::validate_qa_base(endpoints, Some(endpoints.functions)).is_ok());
    assert!(super::validate_qa_base(
        endpoints,
        Some(codeterm_account::firebase_endpoints::PRODUCTION.functions)
    )
    .is_err());
    assert!(super::validate_qa_base(endpoints, Some("http://127.0.0.1:15200/")).is_err());
    assert_eq!(
        super::base_for(true, endpoints, "ignored"),
        endpoints.functions
    );
    assert_eq!(
        super::base_for(false, endpoints, endpoints.functions),
        codeterm_account::firebase_endpoints::PRODUCTION.functions
    );
}
