use super::*;

fn create_params(
    target_installation_id: &str,
    consumer: SecureInputConsumer,
    requester_tab_id: Option<&str>,
) -> SecureInputCreateParams {
    SecureInputCreateParams {
        target_installation_id: target_installation_id.to_owned(),
        target_machine_name: "rollacodebot".to_owned(),
        is_remote: true,
        consumer,
        kind: SecureInputKind::Token,
        reason: "Store repository token on cloud worker".to_owned(),
        requester_tab_id: requester_tab_id.map(str::to_owned),
        ttl_secs: Some(120),
    }
}

#[test]
fn parses_closed_consumers_and_refuses_unknown_or_reserved_targets() {
    assert!(matches!(
        SecureInputConsumer::parse_target("vault:github-deploy", ConsumerParseOptions::default()),
        Ok(SecureInputConsumer::VaultSlot { name, overwrite: false }) if name == "github-deploy"
    ));
    assert!(matches!(
        SecureInputConsumer::parse_target("provider-login:tab-42", ConsumerParseOptions::default()),
        Ok(SecureInputConsumer::ProviderLogin { tab_id, .. }) if tab_id == "tab-42"
    ));
    assert!(matches!(
        SecureInputConsumer::parse_target("provider-key:anthropic", ConsumerParseOptions::default()),
        Ok(SecureInputConsumer::ProviderKey { provider_id, .. }) if provider_id == "anthropic"
    ));

    for bad in [
        "",
        "shell:tab-1",
        "file:/etc/passwd",
        "vault:",
        "vault:domios.instance:api_token",
        "vault:domios:instance:key",
        "vault:codeterm.plugin.bitwarden",
        "provider-login:../other",
        "provider-key:bad/provider",
    ] {
        assert!(
            SecureInputConsumer::parse_target(bad, ConsumerParseOptions::default()).is_err(),
            "expected invalid consumer '{bad}' to be refused"
        );
    }
}

#[test]
fn single_use_claim_blocks_replay_after_delivery_uncertainty_expiry_and_mismatch() {
    let registry = SecureInputRegistry::new();
    let req = registry
        .create(
            create_params(
                "inst-cloud-1",
                SecureInputConsumer::VaultSlot {
                    name: "ci-token".into(),
                    overwrite: false,
                },
                Some("tab-agent"),
            ),
            1_000,
        )
        .expect("request created");

    // Wrong installation is refused without consuming the request.
    assert!(matches!(
        registry.claim_submission(&req.request_id, Some("inst-other"), 1_500),
        Err(SecureInputError::Conflict(_))
    ));

    // First valid claim succeeds; concurrent second claim is refused.
    let claimed = registry
        .claim_submission(&req.request_id, Some("inst-cloud-1"), 1_500)
        .expect("first claim succeeds");
    assert_eq!(claimed.request_id, req.request_id);
    assert!(matches!(
        registry.claim_submission(&req.request_id, Some("inst-cloud-1"), 1_501),
        Err(SecureInputError::Conflict(_))
    ));

    // Settling to Uncertain permanently prevents replay.
    let settled = registry
        .settle_submission(
            &req.request_id,
            SecureInputStatus::Uncertain,
            Some("transport closed after write".into()),
        )
        .expect("settled");
    assert_eq!(settled.status, SecureInputStatus::Uncertain);
    assert!(matches!(
        registry.claim_submission(&req.request_id, Some("inst-cloud-1"), 1_600),
        Err(SecureInputError::Conflict(_))
    ));

    // Expired request transitions to Expired and refuses claim.
    let expiring = registry
        .create(
            create_params(
                "inst-cloud-1",
                SecureInputConsumer::VaultSlot {
                    name: "short-lived".into(),
                    overwrite: false,
                },
                Some("tab-agent"),
            ),
            1_000,
        )
        .expect("expiring request");
    assert!(matches!(
        registry.claim_submission(&expiring.request_id, Some("inst-cloud-1"), 200_000),
        Err(SecureInputError::Conflict(_))
    ));
    assert_eq!(
        registry.get(&expiring.request_id, 200_000).unwrap().status,
        SecureInputStatus::Expired
    );
}

#[test]
fn cancel_is_restricted_to_owner_or_requesting_tab() {
    let registry = SecureInputRegistry::new();
    let req = registry
        .create(
            create_params(
                "inst-local",
                SecureInputConsumer::VaultSlot {
                    name: "deploy-key".into(),
                    overwrite: false,
                },
                Some("tab-owner-agent"),
            ),
            1_000,
        )
        .expect("created");

    assert!(matches!(
        registry.cancel(&req.request_id, Some("tab-foreign"), false, None, 1_100),
        Err(SecureInputError::Forbidden(_))
    ));
    let cancelled = registry
        .cancel(&req.request_id, Some("tab-owner-agent"), false, None, 1_100)
        .expect("requester can cancel");
    assert_eq!(cancelled.status, SecureInputStatus::Refused);
}

#[test]
fn target_dedupe_and_protected_vault_slot_tracking_hold() {
    let registry = SecureInputRegistry::new();
    assert!(registry.claim_target_delivery("sir_once"));
    assert!(!registry.claim_target_delivery("sir_once"));

    assert!(registry.is_protected_vault_slot("domios:provider:anthropic:api_key"));
    assert!(registry.is_protected_vault_slot("domios:git:github"));
    assert!(!registry.is_protected_vault_slot("ordinary-slot"));
    registry.mark_protected_vault_slot("Ordinary-Slot");
    assert!(registry.is_protected_vault_slot("ordinary-slot"));
}

#[test]
fn debug_formatting_and_json_zeroize_never_expose_secret_bytes() {
    let sentinel = "SIR-TOP-SECRET-VALUE-99182";
    let payload = SecureInputDeliveryPayload {
        request_id: "sir_debug_1".into(),
        target_installation_id: "inst-1".into(),
        consumer: SecureInputConsumer::VaultSlot {
            name: "api-token".into(),
            overwrite: true,
        },
        kind: SecureInputKind::Token,
        expires_at_ms: 9_999_999,
        value: SecretString::from(sentinel),
    };
    let debug = format!("{payload:?}");
    assert!(!debug.contains(sentinel));
    assert!(debug.contains("<redacted>"));

    let mut json = serde_json::json!({
        "requestId": "sir_debug_1",
        "value": sentinel,
        "nested": { "token": sentinel },
    });
    zeroize_secure_input_json_value(&mut json);
    assert!(!json.to_string().contains(sentinel));
}
