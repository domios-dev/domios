use super::*;

struct FixtureProof;
#[async_trait]
impl InstallationProofSource for FixtureProof {
    async fn proof(
        &self,
        _session: &Session,
        installation_id: &codeterm_account::InstallationId,
    ) -> Result<codeterm_account::account_mesh::wire::InstallationProof> {
        assert_eq!(installation_id, &id());
        Ok(codeterm_account::account_mesh::wire::InstallationProof {
            schema_version: 1,
            key_id: "1".repeat(64),
            generation: 3,
            timestamp: 1_700_000_000,
            nonce: "nonce".into(),
            signature: "s".repeat(86),
        })
    }
}

#[test]
fn entitlements_attach_proof_and_refuse_unsigned_remote_requests() {
    let proof = codeterm_account::account_mesh::wire::InstallationProof {
        schema_version: 1,
        key_id: "1".repeat(64),
        generation: 3,
        timestamp: 1_700_000_000,
        nonce: "nonce".into(),
        signature: "s".repeat(86),
    };
    let data = entitlements_request(&id(), Some(proof.clone()), false).unwrap();
    assert_eq!(
        data,
        serde_json::json!({"installation_id":"fixture-installation", "installation_proof":proof})
    );
    assert!(entitlements_request(&id(), None, false).is_err());
    assert_eq!(
        entitlements_request(&id(), None, true).unwrap(),
        serde_json::json!({"installation_id":"fixture-installation"})
    );
    assert!(!Cloud::fixture("https://fixture.test/", id()).local_entitlements(&local_session()));
}

#[test]
fn refusal_adapter_ignores_messages_and_invalid_structured_reasons() {
    for body in [
        serde_json::Value::Null,
        serde_json::json!({"error":{"message":"Unknown billing plan"}}),
        serde_json::json!({"error":{"message":"fixture-untrusted", "details":{"reason":"fixture-unknown"}}}),
        serde_json::json!({"error":{"details":{"reason":17}}}),
    ] {
        assert_eq!(refusal_reason(&body), BillingRefusal::Unknown);
    }
    assert_eq!(
        refusal_reason(&serde_json::json!({"error":{"details":{"reason":"unknown_plan"}}})),
        BillingRefusal::UnknownPlan
    );
}

#[test]
fn no_billing_customer_adapter_uses_structured_reason() {
    let body = serde_json::json!({"error": {"status": "FAILED_PRECONDITION",
        "message": "No billing customer", "details": {"reason": "no_billing_customer"}}});
    assert_eq!(refusal_reason(&body), BillingRefusal::NoBillingCustomer);
    assert_eq!(refusal_message(&body), crate::NO_BILLING_CUSTOMER);
}

#[test]
fn plan_operations_pin_callable_names_and_reject_injected_fields() {
    for (input, name, data) in [
        (
            serde_json::json!({"operation":"catalog"}),
            "getAppCatalog",
            serde_json::json!({}),
        ),
        (
            serde_json::json!({"operation":"subscription"}),
            "getSubscriptionPlan",
            serde_json::json!({}),
        ),
        (
            serde_json::json!({"operation":"change","price_id":"pri_fixture"}),
            "changeSubscriptionPlan",
            serde_json::json!({"price_id":"pri_fixture"}),
        ),
    ] {
        let operation: PlanOperation = serde_json::from_value(input).unwrap();
        assert_eq!(operation.callable(), (name, data));
    }
    for input in [
        serde_json::json!({"operation":"catalog","uid":"foreign"}),
        serde_json::json!({"operation":"subscription","uid":"foreign"}),
        serde_json::json!({"operation":"change","price_id":"pri_fixture","uid":"foreign"}),
    ] {
        assert!(serde_json::from_value::<PlanOperation>(input).is_err());
    }
}

fn id() -> codeterm_account::InstallationId {
    codeterm_account::InstallationId::parse("fixture-installation".into()).unwrap()
}

fn local_session() -> Session {
    Session {
        login_generation: 0,
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
        assert_eq!(
            PRODUCTION
                .functions_base(false, Some(endpoint), None)
                .unwrap()
                .unwrap(),
            PRODUCTION.functions
        );
        assert_eq!(
            PRODUCTION
                .functions_base(true, Some(endpoint), None)
                .unwrap()
                .unwrap(),
            endpoint
        );
    }
}

#[tokio::test]
async fn build_endpoint_missing_uses_local_free_and_honest_actions() {
    use codeterm_account::firebase_endpoints::PRODUCTION;
    assert_eq!(PRODUCTION.functions_base(true, None, None), Ok(None));
    let local = super::super::local::Unavailable;
    let session = local_session();
    assert_eq!(
        local.entitlements(&session).await.unwrap(),
        Reading::current(Entitlements::default())
    );
    assert!(local
        .portal_url(&session)
        .await
        .unwrap_err()
        .contains("unavailable"));
}

#[test]
fn qa_billing_requires_the_selected_project() {
    let endpoints = codeterm_account::firebase_endpoints::Endpoints {
        qa: true,
        functions: "https://us-central1-domios-signin-qa.cloudfunctions.net/",
        ..codeterm_account::firebase_endpoints::PRODUCTION
    };
    for saved in [None, Some(endpoints.functions)] {
        assert_eq!(
            endpoints.functions_base(true, saved, None),
            Ok(Some(endpoints.functions))
        );
    }
    for saved in [
        codeterm_account::firebase_endpoints::PRODUCTION.functions,
        "http://127.0.0.1:15200/",
    ] {
        assert!(endpoints.functions_base(true, Some(saved), None).is_err());
    }
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

#[derive(Default)]
struct Revocations(std::sync::Mutex<Vec<u64>>);
#[async_trait]
impl codeterm_account::session_revocation::SessionRevocationObserver for Revocations {
    async fn revoked(&self, session: &Session) {
        self.0.lock().unwrap().push(session.login_generation);
    }
}

#[derive(Default)]
struct RefusingTransport(std::sync::Mutex<Vec<String>>);
#[async_trait]
impl CloudTransport for RefusingTransport {
    async fn execute(&self, request: reqwest::Request) -> Result<(bool, serde_json::Value)> {
        self.0.lock().unwrap().push(request.url().path().to_owned());
        Ok((
            false,
            serde_json::json!({"error":{"status":"UNAUTHENTICATED","details":{"reason":"session_revoked"}}}),
        ))
    }
}

#[tokio::test]
async fn session_revoked_from_entitlements_checkout_portal_and_sponsor_notifies_owner() {
    let observer = std::sync::Arc::new(Revocations::default());
    let transport = std::sync::Arc::new(RefusingTransport::default());
    let mut cloud = Cloud::fixture("https://fixture.test/", id())
        .with_revocations(observer.clone())
        .with_installation_proofs(std::sync::Arc::new(FixtureProof));
    cloud.transport = transport.clone();
    let mut session = local_session();
    session.login_generation = 17;
    assert_eq!(
        cloud.entitlements(&session).await.unwrap_err(),
        "session_revoked"
    );
    assert_eq!(
        cloud
            .checkout_url(&session, "pro_monthly")
            .await
            .unwrap_err(),
        "session_revoked"
    );
    assert_eq!(
        cloud.portal_url(&session).await.unwrap_err(),
        "session_revoked"
    );
    assert_eq!(
        cloud
            .checkout_url(&session, "sponsor_yearly")
            .await
            .unwrap_err(),
        "session_revoked"
    );
    assert_eq!(*observer.0.lock().unwrap(), [17, 17, 17, 17]);
    assert_eq!(
        *transport.0.lock().unwrap(),
        [
            "/getEntitlements",
            "/createCheckoutSession",
            "/createPortalSession",
            "/createCheckoutSession"
        ]
    );
}
