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
fn parses_closed_consumers_and_refuses_unknown_reserved_or_malformed_ids() {
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
        "vault:.",
        "vault:..",
        "vault:domios.instance:api_token",
        "vault:domios:instance:key",
        "vault:codeterm.plugin.bitwarden",
        "provider-login:../other",
        "provider-login:..",
        "provider-key:.",
        "provider-key:..",
        "provider-key:bad/provider",
    ] {
        assert!(
            SecureInputConsumer::parse_target(bad, ConsumerParseOptions::default()).is_err(),
            "expected invalid consumer '{bad}' to be refused"
        );
    }

    // Optional ids with control characters or oversized values must be refused (P2-8).
    assert!(SecureInputConsumer::parse_target(
        "provider-login:tab-1",
        ConsumerParseOptions {
            method_id: Some("bad\nmethod".into()),
            ..ConsumerParseOptions::default()
        }
    )
    .is_err());
    assert!(SecureInputConsumer::parse_target(
        "provider-login:tab-1",
        ConsumerParseOptions {
            provider_id: Some("..".into()),
            ..ConsumerParseOptions::default()
        }
    )
    .is_err());

    // Remote provider-login requires provider_id, login_id, nonce, and epoch (P2-8).
    let incomplete_remote = SecureInputConsumer::ProviderLogin {
        tab_id: "tab-1".into(),
        provider_id: None,
        login_id: Some("login-1".into()),
        session_id: None,
        nonce: Some("nonce-1".into()),
        epoch: Some(1),
        method_id: None,
    };
    assert!(incomplete_remote.validate_remote_requirements().is_err());
}

#[test]
fn single_use_claim_exposes_submitting_and_blocks_replay() {
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

    // First valid claim succeeds and transitions status to Submitting (P1-5).
    let claimed = registry
        .claim_submission(&req.request_id, Some("inst-cloud-1"), 1_500)
        .expect("first claim succeeds");
    assert_eq!(claimed.request_id, req.request_id);
    assert_eq!(claimed.status, SecureInputStatus::Submitting);
    assert_eq!(
        registry.get(&req.request_id, 1_500).unwrap().status,
        SecureInputStatus::Submitting
    );
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
fn settled_churn_never_evicts_pending_or_in_flight_and_per_tab_quota_holds() {
    let registry = SecureInputRegistry::new();
    let owner_pending = registry
        .create(
            create_params(
                "inst-local",
                SecureInputConsumer::VaultSlot {
                    name: "owner-pending".into(),
                    overwrite: false,
                },
                None,
            ),
            1_000,
        )
        .expect("owner pending created");
    let owner_inflight = registry
        .create(
            create_params(
                "inst-local",
                SecureInputConsumer::VaultSlot {
                    name: "owner-inflight".into(),
                    overwrite: false,
                },
                None,
            ),
            1_000,
        )
        .expect("owner inflight created");
    registry
        .claim_submission(&owner_inflight.request_id, Some("inst-local"), 1_100)
        .expect("claimed");

    // Spam create+cancel beyond MAX_RETAINED_REQUESTS from an agent tab (P1-4).
    for i in 0..(MAX_RETAINED_REQUESTS + 20) {
        let req = registry
            .create(
                create_params(
                    "inst-local",
                    SecureInputConsumer::VaultSlot {
                        name: format!("spam-{i}"),
                        overwrite: false,
                    },
                    Some("tab-spammer"),
                ),
                1_200,
            )
            .expect("spam create");
        registry
            .cancel(&req.request_id, Some("tab-spammer"), false, None, 1_200)
            .expect("spam cancel");
    }

    // Both the owner's Pending and in-flight Submitting cards must survive intact!
    assert_eq!(
        registry
            .get(&owner_pending.request_id, 1_300)
            .unwrap()
            .status,
        SecureInputStatus::Pending
    );
    assert_eq!(
        registry
            .get(&owner_inflight.request_id, 1_300)
            .unwrap()
            .status,
        SecureInputStatus::Submitting
    );

    // Per-tab quota bounds active requests per tab while other tabs can still create.
    for i in 0..MAX_PENDING_PER_TAB {
        registry
            .create(
                create_params(
                    "inst-local",
                    SecureInputConsumer::VaultSlot {
                        name: format!("quota-{i}"),
                        overwrite: false,
                    },
                    Some("tab-quota"),
                ),
                1_400,
            )
            .expect("within per-tab quota");
    }
    assert!(matches!(
        registry.create(
            create_params(
                "inst-local",
                SecureInputConsumer::VaultSlot {
                    name: "quota-overflow".into(),
                    overwrite: false,
                },
                Some("tab-quota"),
            ),
            1_400,
        ),
        Err(SecureInputError::Conflict(_))
    ));
    assert!(registry
        .create(
            create_params(
                "inst-local",
                SecureInputConsumer::VaultSlot {
                    name: "other-tab-ok".into(),
                    overwrite: false,
                },
                Some("tab-other"),
            ),
            1_400,
        )
        .is_ok());
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
fn protected_vault_slots_persist_in_host_owned_state_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("secrets")
        .join("protected-vault-slots.json");
    {
        let registry = SecureInputRegistry::new_at(Some(path.clone()));
        assert!(registry.claim_target_delivery("sir_once"));
        assert!(!registry.claim_target_delivery("sir_once"));
        assert!(registry.is_protected_vault_slot("domios:provider:anthropic:api_key"));
        assert!(registry.is_protected_vault_slot("domios:git:github"));
        assert!(!registry.is_protected_vault_slot("ordinary-slot"));
        registry.mark_protected_vault_slot("Ordinary-Slot");
        assert!(registry.is_protected_vault_slot("ordinary-slot"));
    }
    // Reload from disk into a fresh registry instance (P0-2).
    let reloaded = SecureInputRegistry::new_at(Some(path));
    assert!(reloaded.is_protected_vault_slot("ordinary-slot"));
    assert!(reloaded.is_protected_vault_slot("ORDINARY-SLOT"));
}

#[test]
fn debug_formatting_scrubbing_and_json_zeroize_never_expose_secret_bytes() {
    let raw_secret = "  SIR-TOP-SECRET-VALUE-99182  ";
    let trimmed = raw_secret.trim();
    let payload = SecureInputDeliveryPayload {
        request_id: "sir_debug_1".into(),
        target_installation_id: "inst-1".into(),
        consumer: SecureInputConsumer::VaultSlot {
            name: "api-token".into(),
            overwrite: true,
        },
        kind: SecureInputKind::Token,
        expires_at_ms: 9_999_999,
        value: SecretString::from(raw_secret),
    };
    let debug = format!("{payload:?}");
    assert!(!debug.contains(trimmed));
    assert!(debug.contains("<redacted>"));

    // Scrubbing removes both raw and trimmed variants (P2-10).
    let scrubbed_raw = scrub_secret(&format!("failed for [{raw_secret}]"), raw_secret);
    assert!(!scrubbed_raw.contains(trimmed));
    let scrubbed_trimmed = scrub_secret(&format!("failed for {trimmed}"), raw_secret);
    assert!(!scrubbed_trimmed.contains(trimmed));

    let mut json = serde_json::json!({
        "requestId": "sir_debug_1",
        "value": trimmed,
        "nested": { "token": trimmed },
    });
    zeroize_secure_input_json_value(&mut json);
    assert!(!json.to_string().contains(trimmed));
}
