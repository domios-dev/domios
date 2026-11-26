use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::account_mesh::{
    callable::{refusal, CallableFailure},
    cloud_enrollment::{
        enrollment_message, fixed_bytes, nonce_commitment, EnrollmentReply, EnrollmentRequest,
    },
    cloud_provisioning::{
        provisioning_message, CloudEndpoint, CloudProvisioningRefusal, CreateCloudMachineRequest,
        DeleteMachineReply, ManagedCloudMachineActionRequest, ProvisionedMachineReply,
        SleepMachineReply, WireCloudEndpoint, WireCloudSleepMode, CLOUD_INGRESS_PORT,
        DOMAIN_CLOUD_CREATE, DOMAIN_CLOUD_DELETE, DOMAIN_CLOUD_SLEEP, DOMAIN_CLOUD_WAKE,
        PROVIDER_PROXY_PORT, ROUTE_CLOUD_CREATE, ROUTE_CLOUD_DELETE, ROUTE_CLOUD_SLEEP,
        ROUTE_CLOUD_WAKE,
    },
    delegated_renewal::{RenewalReply, RenewalRequest},
    wire::RegistryRefusal,
};

const FIXTURES_JSON: &str = include_str!("cloud_provisioning_fixtures.json");

#[derive(Deserialize)]
struct FixtureFile {
    schema: String,
    test_context: FixtureContext,
    routes: BTreeMap<String, String>,
    domains: BTreeMap<String, String>,
    requests: FixtureRequests,
    replies: FixtureReplies,
    refusals: BTreeMap<String, FixtureRefusal>,
}

#[derive(Deserialize)]
struct FixtureContext {
    project: String,
    account_id: String,
    controller_key: String,
    bootstrap_nonce_sha256: String,
}

#[derive(Deserialize)]
struct FixtureRequests {
    create: CreateCloudMachineRequest,
    wake: ManagedCloudMachineActionRequest,
    sleep: ManagedCloudMachineActionRequest,
    delete: ManagedCloudMachineActionRequest,
    enroll: EnrollmentRequest,
    renew: RenewalRequest,
}

#[derive(Deserialize)]
struct FixtureReplies {
    create: ProvisionedMachineReply,
    create_provider_proxy: ProvisionedMachineReply,
    wake: ProvisionedMachineReply,
    wake_provider_proxy: ProvisionedMachineReply,
    sleep: SleepMachineReply,
    delete: DeleteMachineReply,
    enroll: EnrollmentReply,
    renew: RenewalReply,
}

#[derive(Deserialize)]
struct FixtureRefusal {
    http_status: u16,
    grpc_status: String,
    details: Value,
}

fn verify_fixture_sig(controller: &VerifyingKey, message: &[u8], sig_b64: &str) {
    let raw = fixed_bytes::<64>(sig_b64).expect("64-byte base64url signature");
    controller
        .verify_strict(message, &Signature::from_bytes(&raw))
        .expect("valid Ed25519 signature over canonical JCS fixture payload");
}

#[test]
fn decodes_and_verifies_all_ledger_cloud_provisioning_request_and_reply_fixtures() {
    let fixtures: FixtureFile =
        serde_json::from_str(FIXTURES_JSON).expect("decode shared/cloud-provisioning fixtures");
    assert_eq!(fixtures.schema, "domios.cloud-provisioning.fixtures.v1");

    assert_eq!(fixtures.routes["create"], ROUTE_CLOUD_CREATE);
    assert_eq!(fixtures.routes["wake"], ROUTE_CLOUD_WAKE);
    assert_eq!(fixtures.routes["sleep"], ROUTE_CLOUD_SLEEP);
    assert_eq!(fixtures.routes["delete"], ROUTE_CLOUD_DELETE);

    assert_eq!(fixtures.domains["create"], DOMAIN_CLOUD_CREATE);
    assert_eq!(fixtures.domains["wake"], DOMAIN_CLOUD_WAKE);
    assert_eq!(fixtures.domains["sleep"], DOMAIN_CLOUD_SLEEP);
    assert_eq!(fixtures.domains["delete"], DOMAIN_CLOUD_DELETE);

    let key_bytes = fixed_bytes::<32>(&fixtures.test_context.controller_key).unwrap();
    let controller = VerifyingKey::from_bytes(&key_bytes).unwrap();
    let project = &fixtures.test_context.project;
    let account = &fixtures.test_context.account_id;

    // Verify PSK -> bootstrap_nonce_sha256 commitment parity with Baker.
    let psk_bytes = fixed_bytes::<32>(&fixtures.requests.create.body.creation_psk).unwrap();
    assert_eq!(
        nonce_commitment(&psk_bytes),
        fixtures.test_context.bootstrap_nonce_sha256
    );
    // Redacted Debug never leaks the creation PSK.
    let debug_text = format!("{:?}", fixtures.requests.create.body);
    assert!(!debug_text.contains(&fixtures.requests.create.body.creation_psk));

    // Cryptographically verify all 6 JCS request envelopes signed by Ledger's fixture generator.
    verify_fixture_sig(
        &controller,
        &provisioning_message(
            DOMAIN_CLOUD_CREATE,
            project,
            account,
            &fixtures.requests.create.body,
        )
        .unwrap(),
        &fixtures.requests.create.controller_signature,
    );
    verify_fixture_sig(
        &controller,
        &provisioning_message(
            DOMAIN_CLOUD_WAKE,
            project,
            account,
            &fixtures.requests.wake.body,
        )
        .unwrap(),
        &fixtures.requests.wake.controller_signature,
    );
    verify_fixture_sig(
        &controller,
        &provisioning_message(
            DOMAIN_CLOUD_SLEEP,
            project,
            account,
            &fixtures.requests.sleep.body,
        )
        .unwrap(),
        &fixtures.requests.sleep.controller_signature,
    );
    verify_fixture_sig(
        &controller,
        &provisioning_message(
            DOMAIN_CLOUD_DELETE,
            project,
            account,
            &fixtures.requests.delete.body,
        )
        .unwrap(),
        &fixtures.requests.delete.controller_signature,
    );
    verify_fixture_sig(
        &controller,
        &enrollment_message(project, account, &fixtures.requests.enroll.body).unwrap(),
        &fixtures.requests.enroll.controller_signature,
    );
    verify_fixture_sig(
        &controller,
        &provisioning_message(
            "domios.cloud-renew.v1",
            project,
            account,
            &fixtures.requests.renew.body,
        )
        .unwrap(),
        &fixtures.requests.renew.controller_signature,
    );

    // Verify all reply fixtures decode with expected endpoints, sleep_mode, and rates.
    assert_eq!(
        fixtures.replies.create.validated_endpoint(),
        Some(CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT
        )))
    );
    assert_eq!(
        fixtures.replies.create.sleep_mode,
        WireCloudSleepMode::DeleteRecreate
    );
    assert_eq!(fixtures.replies.create.rate.basis, "managed");
    assert_eq!(fixtures.replies.create.rate.credits_per_hour, 1.0);

    // Provider-proxy create and wake replies carry the stable lowercase DNS host over wss/443.
    assert_eq!(
        fixtures.replies.create_provider_proxy.validated_endpoint(),
        Some(CloudEndpoint::ProviderProxy {
            host: "domios-a1b2c3d4.fly.dev".into(),
        })
    );
    assert_eq!(
        fixtures.replies.create_provider_proxy.sleep_mode,
        WireCloudSleepMode::StopBilled
    );
    assert_eq!(
        fixtures.replies.wake_provider_proxy.machine_id,
        fixtures.replies.create_provider_proxy.machine_id
    );
    assert_eq!(
        fixtures.replies.wake_provider_proxy.validated_endpoint(),
        Some(CloudEndpoint::ProviderProxy {
            host: "domios-a1b2c3d4.fly.dev".into(),
        })
    );
    assert_eq!(
        fixtures.replies.wake_provider_proxy.sleep_mode,
        WireCloudSleepMode::StopBilled
    );

    // delete_recreate wake returns a new IPv4 on the same machine_id.
    assert_eq!(
        fixtures.replies.wake.machine_id,
        fixtures.replies.create.machine_id
    );
    assert_eq!(
        fixtures.replies.wake.validated_endpoint(),
        Some(CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 42),
            CLOUD_INGRESS_PORT
        )))
    );
    assert_eq!(fixtures.replies.sleep.state, "asleep");
    assert_eq!(
        fixtures.replies.sleep.sleep_mode,
        WireCloudSleepMode::DeleteRecreate
    );
    assert_eq!(fixtures.replies.delete.state, "deleted");
    assert!(!fixtures.replies.enroll.signed_registry.signature.is_empty());
    assert!(!fixtures.replies.renew.signed_registry.signature.is_empty());
}

#[test]
fn decodes_all_ledger_cloud_provisioning_refusal_fixtures() {
    let fixtures: FixtureFile =
        serde_json::from_str(FIXTURES_JSON).expect("decode shared/cloud-provisioning fixtures");
    assert_eq!(fixtures.refusals.len(), 22);

    let decode_http = |name: &str| -> (CloudProvisioningRefusal, RegistryRefusal) {
        let entry = &fixtures.refusals[name];
        let typed: CloudProvisioningRefusal = serde_json::from_value(entry.details.clone())
            .unwrap_or_else(|e| panic!("decode CloudProvisioningRefusal fixture {name}: {e}"));
        let failure = CallableFailure::Status {
            status: entry.http_status,
            retry_after_seconds: None,
            body: Some(json!({
                "error": {
                    "status": entry.grpc_status,
                    "details": entry.details,
                }
            })),
        };
        (typed, refusal(&failure))
    };

    assert_eq!(
        decode_http("invalid"),
        (CloudProvisioningRefusal::Invalid, RegistryRefusal::Invalid)
    );
    assert_eq!(
        decode_http("unauthenticated"),
        (
            CloudProvisioningRefusal::Unauthenticated,
            RegistryRefusal::Unauthenticated
        )
    );
    assert_eq!(
        decode_http("app_check"),
        (
            CloudProvisioningRefusal::AppCheck,
            RegistryRefusal::AppCheck
        )
    );
    assert_eq!(
        decode_http("session_revoked"),
        (
            CloudProvisioningRefusal::SessionRevoked,
            RegistryRefusal::Unauthenticated
        )
    );
    assert_eq!(
        decode_http("forbidden"),
        (
            CloudProvisioningRefusal::Forbidden,
            RegistryRefusal::Forbidden
        )
    );
    assert_eq!(
        decode_http("revoked"),
        (CloudProvisioningRefusal::Revoked, RegistryRefusal::Revoked)
    );
    assert_eq!(
        decode_http("proof_invalid"),
        (
            CloudProvisioningRefusal::ProofInvalid,
            RegistryRefusal::ProofInvalid
        )
    );
    assert_eq!(
        decode_http("cloud_machines_free"),
        (
            CloudProvisioningRefusal::CloudMachines {
                limit: 0,
                active_count: 0,
            },
            RegistryRefusal::CloudMachineLimit {
                limit: 0,
                active_count: 0,
            }
        )
    );
    assert_eq!(
        decode_http("cloud_machines_pro"),
        (
            CloudProvisioningRefusal::CloudMachines {
                limit: 1,
                active_count: 1,
            },
            RegistryRefusal::CloudMachineLimit {
                limit: 1,
                active_count: 1,
            }
        )
    );
    assert_eq!(
        decode_http("lease_required"),
        (
            CloudProvisioningRefusal::LeaseRequired,
            RegistryRefusal::LeaseRequired
        )
    );
    assert_eq!(
        decode_http("credits_exhausted"),
        (
            CloudProvisioningRefusal::CreditsExhausted,
            RegistryRefusal::CreditsExhausted
        )
    );
    assert_eq!(
        decode_http("not_found"),
        (
            CloudProvisioningRefusal::NotFound,
            RegistryRefusal::NotFound
        )
    );
    assert_eq!(
        decode_http("conflict"),
        (
            CloudProvisioningRefusal::Conflict,
            RegistryRefusal::Conflict
        )
    );
    assert_eq!(
        decode_http("replay"),
        (CloudProvisioningRefusal::Replay, RegistryRefusal::Replay)
    );
    assert_eq!(
        decode_http("rate_limited"),
        (
            CloudProvisioningRefusal::RateLimited {
                retry_after_seconds: 60,
            },
            RegistryRefusal::RateLimited {
                retry_after_seconds: 60,
            }
        )
    );
    assert_eq!(
        decode_http("endpoint_kind_unsupported"),
        (
            CloudProvisioningRefusal::EndpointKindUnsupported,
            RegistryRefusal::EndpointKindUnsupported
        )
    );
    assert_eq!(
        refusal(&CallableFailure::Status {
            status: fixtures.refusals["apply_too_large"].http_status,
            retry_after_seconds: None,
            body: Some(json!({
                "error": {
                    "status": fixtures.refusals["apply_too_large"].grpc_status,
                    "details": fixtures.refusals["apply_too_large"].details,
                }
            })),
        }),
        RegistryRefusal::ApplyTooLarge
    );
    assert_eq!(
        decode_http("provider_refused"),
        (
            CloudProvisioningRefusal::ProviderRefused {
                provider_reason: Some("quota".into()),
            },
            RegistryRefusal::ProviderRefused
        )
    );
    assert_eq!(
        decode_http("provider_unavailable"),
        (
            CloudProvisioningRefusal::ProviderUnavailable,
            RegistryRefusal::ProviderUnavailable
        )
    );
    assert_eq!(
        decode_http("provider_unconfirmed"),
        (
            CloudProvisioningRefusal::ProviderUnconfirmed,
            RegistryRefusal::ProviderUnconfirmed
        )
    );
    assert_eq!(
        decode_http("authority_unavailable"),
        (
            CloudProvisioningRefusal::AuthorityUnavailable,
            RegistryRefusal::AuthorityUnavailable
        )
    );
    assert_eq!(
        decode_http("unavailable"),
        (
            CloudProvisioningRefusal::Unavailable,
            RegistryRefusal::Unavailable
        )
    );
}

#[test]
fn decodes_and_validates_redeem_cloud_credit_promo_wire_types_and_refusals() {
    use crate::account_mesh::{
        cloud_provisioning::{
            PromoRedeemRefusal, RedeemCloudCreditPromoReply, RedeemCloudCreditPromoRequest,
            ROUTE_CLOUD_REDEEM_PROMO,
        },
        state::AccountMeshRefusal,
    };

    assert_eq!(ROUTE_CLOUD_REDEEM_PROMO, "redeemCloudCreditPromo");

    let req: RedeemCloudCreditPromoRequest =
        serde_json::from_value(json!({ "code": "DOMIOS-BETA-25" })).unwrap();
    assert_eq!(req.code, "DOMIOS-BETA-25");
    assert!(
        serde_json::from_value::<RedeemCloudCreditPromoRequest>(
            json!({ "code": "DOMIOS-BETA-25", "extra": true })
        )
        .is_err(),
        "request must reject unknown fields"
    );

    let fresh: RedeemCloudCreditPromoReply = serde_json::from_value(json!({
        "schema_version": 1,
        "status": "redeemed",
        "already_redeemed": false,
        "hours": 25.0,
        "expires_at": 1_800_000_000u64,
        "remaining": 35.0,
        "purchased": 25.0
    }))
    .unwrap();
    assert!(fresh.is_valid());
    assert!(!fresh.already_redeemed);
    assert_eq!(fresh.expires_at, Some(1_800_000_000));

    let idempotent: RedeemCloudCreditPromoReply = serde_json::from_value(json!({
        "schema_version": 1,
        "status": "redeemed",
        "already_redeemed": true,
        "hours": 25.0,
        "expires_at": null,
        "remaining": 35.0,
        "purchased": 25.0
    }))
    .unwrap();
    assert!(idempotent.is_valid());
    assert!(idempotent.already_redeemed);
    assert_eq!(idempotent.expires_at, None);

    // Reject unknown fields and missing required fields on reply.
    assert!(
        serde_json::from_value::<RedeemCloudCreditPromoReply>(json!({
            "schema_version": 1,
            "status": "redeemed",
            "already_redeemed": false,
            "hours": 25.0,
            "expires_at": null,
            "remaining": 35.0,
            "purchased": 25.0,
            "unexpected": 1
        }))
        .is_err()
    );

    // All 6 typed promo refusal reasons decode strictly and map through callable::refusal.
    for (reason, expected_wire, expected_registry, expected_mesh) in [
        (
            "invalid_code",
            PromoRedeemRefusal::InvalidCode,
            RegistryRefusal::PromoInvalidCode,
            AccountMeshRefusal::PromoInvalidCode,
        ),
        (
            "expired",
            PromoRedeemRefusal::Expired,
            RegistryRefusal::PromoExpired,
            AccountMeshRefusal::PromoExpired,
        ),
        (
            "disabled",
            PromoRedeemRefusal::Disabled,
            RegistryRefusal::PromoDisabled,
            AccountMeshRefusal::PromoDisabled,
        ),
        (
            "exhausted",
            PromoRedeemRefusal::Exhausted,
            RegistryRefusal::PromoExhausted,
            AccountMeshRefusal::PromoExhausted,
        ),
        (
            "spend_cap_exceeded",
            PromoRedeemRefusal::SpendCapExceeded,
            RegistryRefusal::PromoSpendCapExceeded,
            AccountMeshRefusal::PromoSpendCapExceeded,
        ),
    ] {
        let details = json!({ "reason": reason });
        assert_eq!(
            serde_json::from_value::<PromoRedeemRefusal>(details.clone()).unwrap(),
            expected_wire
        );
        let mapped = refusal(&CallableFailure::Status {
            status: 400,
            retry_after_seconds: None,
            body: Some(json!({ "error": { "status": "FAILED_PRECONDITION", "details": details } })),
        });
        assert_eq!(mapped, expected_registry);
        assert_eq!(AccountMeshRefusal::from(mapped), expected_mesh);
    }

    // rate_limited with and without retry_after_seconds.
    assert_eq!(
        serde_json::from_value::<PromoRedeemRefusal>(json!({ "reason": "rate_limited" })).unwrap(),
        PromoRedeemRefusal::RateLimited {
            retry_after_seconds: None
        }
    );
    assert_eq!(
        serde_json::from_value::<PromoRedeemRefusal>(
            json!({ "reason": "rate_limited", "retry_after_seconds": 120 })
        )
        .unwrap(),
        PromoRedeemRefusal::RateLimited {
            retry_after_seconds: Some(120)
        }
    );
    assert_eq!(
        refusal(&CallableFailure::Status {
            status: 429,
            retry_after_seconds: Some(90),
            body: Some(json!({
                "error": { "status": "RESOURCE_EXHAUSTED", "details": { "reason": "rate_limited" } }
            })),
        }),
        RegistryRefusal::RateLimited {
            retry_after_seconds: 90
        }
    );
    assert_eq!(
        RegistryRefusal::from_details(&json!({ "reason": "rate_limited" })),
        RegistryRefusal::RateLimited {
            retry_after_seconds: 1
        }
    );
}

#[test]
fn endpoint_union_refuses_unknown_kind_ip_host_wrong_ports_and_non_wss_scheme() {
    let base_reply = |endpoint: Value| {
        json!({
            "machine_id": "cm_test_1",
            "provider": "fly_machines",
            "endpoint": endpoint,
            "size": "small",
            "rate": {
                "amount": 0.2,
                "currency": "USD",
                "basis": "managed",
                "credits_per_hour": 1.0
            },
            "sleep_mode": "stop_billed",
            "state": "awake",
            "lease_id": "lease_1",
            "lease_expires_at": 1795700600u64
        })
    };

    // Unknown kind, extra fields, and legacy top-level ipv4/port fail deserialization.
    assert!(serde_json::from_value::<WireCloudEndpoint>(json!({
        "kind": "relay_tunnel",
        "host": "domios-a.fly.dev",
        "port": 443,
        "scheme": "wss"
    }))
    .is_err());
    assert!(serde_json::from_value::<WireCloudEndpoint>(json!({
        "kind": "provider_proxy",
        "host": "domios-a.fly.dev",
        "port": 443,
        "scheme": "wss",
        "extra": true
    }))
    .is_err());
    assert!(serde_json::from_value::<ProvisionedMachineReply>(json!({
        "machine_id": "cm_legacy",
        "provider": "timeweb",
        "ipv4": "198.51.100.10",
        "port": 7687,
        "size": "small",
        "rate": {
            "amount": 0.2,
            "currency": "USD",
            "basis": "managed",
            "credits_per_hour": 1.0
        },
        "sleep_mode": "delete_recreate",
        "state": "awake",
        "lease_id": "lease_legacy",
        "lease_expires_at": 1795700600u64
    }))
    .is_err());

    let too_long_host = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(62)
    );
    assert_eq!(too_long_host.len(), 254);

    for bad_endpoint in [
        // IP literals as provider_proxy host
        json!({"kind": "provider_proxy", "host": "198.51.100.10", "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": "127.0.0.1", "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": "::1", "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": "[::1]", "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        // Uppercase or oversized or port-bearing host
        json!({"kind": "provider_proxy", "host": "Domios-App.fly.dev", "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev:443", "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": too_long_host, "port": PROVIDER_PROXY_PORT, "scheme": "wss"}),
        // Wrong port for provider_proxy
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev", "port": 7687, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev", "port": 8443, "scheme": "wss"}),
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev", "port": 80, "scheme": "wss"}),
        // Non-wss scheme for provider_proxy
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev", "port": PROVIDER_PROXY_PORT, "scheme": "ws"}),
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev", "port": PROVIDER_PROXY_PORT, "scheme": "https"}),
        json!({"kind": "provider_proxy", "host": "domios-app.fly.dev", "port": PROVIDER_PROXY_PORT, "scheme": "http"}),
        // Wrong port or non-IPv4 for dedicated_ipv4
        json!({"kind": "dedicated_ipv4", "ipv4": "198.51.100.10", "port": 443}),
        json!({"kind": "dedicated_ipv4", "ipv4": "domios-app.fly.dev", "port": CLOUD_INGRESS_PORT}),
    ] {
        let reply: ProvisionedMachineReply =
            serde_json::from_value(base_reply(bad_endpoint.clone())).unwrap();
        assert_eq!(
            reply.validated_endpoint(),
            None,
            "expected endpoint refusal for {bad_endpoint}"
        );
    }
}
