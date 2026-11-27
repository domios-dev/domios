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
        provisioning_message, CloudCheckoutRefusal, CloudEndpoint, CloudProvisioningRefusal,
        CloudUsageReply, CreateCloudCreditPackCheckoutReply, CreateCloudCreditPackCheckoutRequest,
        CreateCloudMachineRequest, DeleteMachineReply, ManagedCloudMachineActionRequest,
        ProvisionedMachineReply, SleepMachineReply, WireCloudEndpoint, WireCloudPlanAllowances,
        WireCloudSleepMode, WireCloudUsagePack, WireCloudUsageSize, CLOUD_INGRESS_PORT,
        DOMAIN_CLOUD_ARCHIVE, DOMAIN_CLOUD_CREATE, DOMAIN_CLOUD_DELETE, DOMAIN_CLOUD_SLEEP,
        DOMAIN_CLOUD_WAKE, PROVIDER_PROXY_PORT, ROUTE_CLOUD_ARCHIVE, ROUTE_CLOUD_CHECKOUT,
        ROUTE_CLOUD_CREATE, ROUTE_CLOUD_DELETE, ROUTE_CLOUD_SLEEP, ROUTE_CLOUD_USAGE,
        ROUTE_CLOUD_WAKE,
    },
    delegated_renewal::{RenewalReply, RenewalRequest},
    state::AccountMeshRefusal,
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
    create_pack_checkout: CreateCloudCreditPackCheckoutRequest,
    create_pack_checkout_custom: CreateCloudCreditPackCheckoutRequest,
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
    create_pack_checkout: CreateCloudCreditPackCheckoutReply,
    usage: CloudUsageReply,
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
    assert_eq!(fixtures.routes["archiveNow"], ROUTE_CLOUD_ARCHIVE);
    assert_eq!(fixtures.routes["delete"], ROUTE_CLOUD_DELETE);
    assert_eq!(fixtures.routes["usage"], ROUTE_CLOUD_USAGE);
    assert_eq!(
        fixtures.routes["create_pack_checkout"],
        ROUTE_CLOUD_CHECKOUT
    );

    assert_eq!(fixtures.domains["create"], DOMAIN_CLOUD_CREATE);
    assert_eq!(fixtures.domains["wake"], DOMAIN_CLOUD_WAKE);
    assert_eq!(fixtures.domains["sleep"], DOMAIN_CLOUD_SLEEP);
    assert_eq!(fixtures.domains["archive"], DOMAIN_CLOUD_ARCHIVE);
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
        RegistryRefusal::from_details(&serde_json::json!({ "reason": "provider_starting" })),
        RegistryRefusal::ProviderStarting
    );
    assert_eq!(
        serde_json::from_value::<CloudProvisioningRefusal>(serde_json::json!({
            "reason": "provider_starting"
        }))
        .unwrap(),
        CloudProvisioningRefusal::ProviderStarting
    );
    assert_eq!(
        AccountMeshRefusal::from(RegistryRefusal::ProviderStarting),
        AccountMeshRefusal::ProviderUnconfirmed
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

    // Replies tolerate additive backend fields so shipped desktops survive backend upgrades.
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
        .is_ok()
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
fn decodes_and_validates_cloud_commerce_checkout_and_usage_fixtures_and_refusals() {
    use crate::account_mesh::state::AccountMeshRefusal;

    let fixtures: FixtureFile =
        serde_json::from_str(FIXTURES_JSON).expect("decode shared/cloud-provisioning fixtures");

    assert_eq!(ROUTE_CLOUD_CHECKOUT, "createCloudCreditPackCheckout");
    assert_eq!(ROUTE_CLOUD_USAGE, "getManagedCloudUsage");

    assert_eq!(
        fixtures.requests.create_pack_checkout,
        CreateCloudCreditPackCheckoutRequest {
            pack_id: "pack_5".into(),
            amount_cents: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&fixtures.requests.create_pack_checkout).unwrap(),
        json!({ "pack_id": "pack_5" })
    );

    assert_eq!(
        fixtures.requests.create_pack_checkout_custom,
        CreateCloudCreditPackCheckoutRequest {
            pack_id: "custom".into(),
            amount_cents: Some(700),
        }
    );
    assert_eq!(
        serde_json::to_value(&fixtures.requests.create_pack_checkout_custom).unwrap(),
        json!({ "pack_id": "custom", "amount_cents": 700 })
    );

    assert!(
        serde_json::from_value::<CreateCloudCreditPackCheckoutRequest>(json!({
            "pack_id": "pack_5",
            "extra": true
        }))
        .is_err()
    );

    let checkout_reply = &fixtures.replies.create_pack_checkout;
    assert!(checkout_reply.is_valid());
    assert_eq!(
        checkout_reply.url,
        "https://checkout.stripe.com/c/pay/cs_test_fixture_pack_5"
    );
    assert_eq!(checkout_reply.session_id, "cs_test_fixture_pack_5");
    assert_eq!(checkout_reply.pack_id, "pack_5");
    assert_eq!(checkout_reply.credits, 25.0);
    assert_eq!(checkout_reply.amount_cents, 500);
    assert_eq!(checkout_reply.currency, "USD");

    assert!(
        serde_json::from_value::<CreateCloudCreditPackCheckoutReply>(json!({
            "url": "https://checkout.stripe.com/c/pay/cs_test_fixture_pack_5",
            "session_id": "cs_test_fixture_pack_5",
            "pack_id": "pack_5",
            "credits": 25,
            "amount_cents": 500,
            "currency": "USD",
            "unexpected": true
        }))
        .is_ok()
    );

    let mut bad_url = checkout_reply.clone();
    bad_url.url = "http://checkout.stripe.com/c/pay/cs_test".into();
    assert!(!bad_url.is_valid());
    let mut bad_amount = checkout_reply.clone();
    bad_amount.amount_cents = 0;
    assert!(!bad_amount.is_valid());
    let mut bad_credits = checkout_reply.clone();
    bad_credits.credits = -1.0;
    assert!(!bad_credits.is_valid());

    let usage = &fixtures.replies.usage;
    assert_eq!(usage.used, 1.5);
    assert_eq!(usage.allowance, 10.0);
    assert_eq!(usage.purchased, 25.0);
    assert_eq!(usage.remaining, 33.5);
    assert_eq!(usage.cap, 50.0);
    assert_eq!(usage.burn_per_size.get("small"), Some(&1.0));
    assert_eq!(usage.currency.as_deref(), Some("USD"));
    assert_eq!(usage.period_end_at, Some(1_798_761_600));
    assert_eq!(usage.custom_min_cents, Some(500));
    assert_eq!(usage.custom_max_cents, Some(4500));
    assert_eq!(usage.plan.as_deref(), Some("pro"));
    assert_eq!(
        usage.sizes.as_deref(),
        Some(
            &[WireCloudUsageSize {
                id: "small".into(),
                cpu: 2,
                ram_gb: 4.0,
                hourly_rate_cents: 20,
                credits_per_hour: 1.0,
            }][..]
        )
    );
    assert_eq!(
        usage.packs.as_deref(),
        Some(
            &[
                WireCloudUsagePack {
                    pack_id: "pack_5".into(),
                    price_cents: 500,
                    hours: 25.0,
                },
                WireCloudUsagePack {
                    pack_id: "pack_10".into(),
                    price_cents: 1000,
                    hours: 50.0,
                },
                WireCloudUsagePack {
                    pack_id: "pack_25".into(),
                    price_cents: 2500,
                    hours: 125.0,
                },
            ][..]
        )
    );
    assert_eq!(usage.purchased_expiry_days, Some(365));
    assert_eq!(
        usage.allowance_hours,
        Some(WireCloudPlanAllowances {
            free: 0.0,
            pro: 10.0,
            max: 40.0,
            sponsor: 120.0,
        })
    );

    // Optional catalog fields may be omitted on older backend replies.
    let minimal_usage: CloudUsageReply = serde_json::from_value(json!({
        "used": 0.0,
        "allowance": 10.0,
        "purchased": 0.0,
        "remaining": 10.0,
        "cap": 50.0,
        "burn_per_size": {}
    }))
    .unwrap();
    assert_eq!(minimal_usage.custom_min_cents, None);
    assert_eq!(minimal_usage.plan, None);
    assert_eq!(minimal_usage.sizes, None);
    assert_eq!(minimal_usage.packs, None);
    assert_eq!(minimal_usage.purchased_expiry_days, None);
    assert_eq!(minimal_usage.allowance_hours, None);

    // Newer servers may add fields; usage and nested catalog replies tolerate them.
    assert!(serde_json::from_value::<CloudUsageReply>(json!({
        "used": 0.0,
        "allowance": 10.0,
        "purchased": 0.0,
        "remaining": 10.0,
        "cap": 50.0,
        "burn_per_size": {},
        "extra": true
    }))
    .is_ok());
    assert!(serde_json::from_value::<WireCloudUsageSize>(json!({
        "id": "small",
        "cpu": 2,
        "ram_gb": 4,
        "hourly_rate_cents": 20,
        "credits_per_hour": 1,
        "extra": 1
    }))
    .is_ok());

    // Spend cap exceeded with Tally's 4 extra cent fields decodes strictly and maps cleanly.
    let cap_details = json!({
        "reason": "spend_cap_exceeded",
        "cap_cents": 5000,
        "spent_cents": 4500,
        "pending_cents": 0,
        "pack_cents": 1000
    });
    assert_eq!(
        serde_json::from_value::<CloudCheckoutRefusal>(cap_details.clone()).unwrap(),
        CloudCheckoutRefusal::SpendCapExceeded {
            cap_cents: Some(5000),
            spent_cents: Some(4500),
            pending_cents: Some(0),
            pack_cents: Some(1000),
        }
    );
    let cap_mapped = refusal(&CallableFailure::Status {
        status: 400,
        retry_after_seconds: None,
        body: Some(json!({
            "error": { "status": "FAILED_PRECONDITION", "details": cap_details }
        })),
    });
    assert_eq!(cap_mapped, RegistryRefusal::PromoSpendCapExceeded);
    assert_eq!(
        AccountMeshRefusal::from(cap_mapped),
        AccountMeshRefusal::PromoSpendCapExceeded
    );

    // Unknown fields on spend_cap_exceeded details are rejected as unavailable.
    assert_eq!(
        RegistryRefusal::from_details(&json!({
            "reason": "spend_cap_exceeded",
            "unexpected_cents": 1
        })),
        RegistryRefusal::Unavailable
    );

    // All typed checkout refusal reasons decode strictly and map through callable::refusal.
    for (status, grpc_status, reason, expected_wire, expected_registry, expected_mesh) in [
        (
            400,
            "INVALID_ARGUMENT",
            "invalid",
            CloudCheckoutRefusal::Invalid,
            RegistryRefusal::Invalid,
            AccountMeshRefusal::Invalid,
        ),
        (
            400,
            "FAILED_PRECONDITION",
            "below_min",
            CloudCheckoutRefusal::BelowMin,
            RegistryRefusal::CheckoutBelowMin,
            AccountMeshRefusal::CheckoutBelowMin,
        ),
        (
            400,
            "FAILED_PRECONDITION",
            "above_max",
            CloudCheckoutRefusal::AboveMax,
            RegistryRefusal::CheckoutAboveMax,
            AccountMeshRefusal::CheckoutAboveMax,
        ),
        (
            403,
            "PERMISSION_DENIED",
            "territory",
            CloudCheckoutRefusal::Territory,
            RegistryRefusal::CheckoutTerritory,
            AccountMeshRefusal::CheckoutTerritory,
        ),
        (
            503,
            "UNAVAILABLE",
            "unavailable",
            CloudCheckoutRefusal::Unavailable,
            RegistryRefusal::Unavailable,
            AccountMeshRefusal::Unavailable,
        ),
    ] {
        let details = json!({ "reason": reason });
        assert_eq!(
            serde_json::from_value::<CloudCheckoutRefusal>(details.clone()).unwrap(),
            expected_wire
        );
        let mapped = refusal(&CallableFailure::Status {
            status,
            retry_after_seconds: None,
            body: Some(json!({ "error": { "status": grpc_status, "details": details } })),
        });
        assert_eq!(mapped, expected_registry, "{reason}");
        assert_eq!(AccountMeshRefusal::from(mapped), expected_mesh, "{reason}");
    }

    // Unconfigured Stripe checkout returns HTTP 400 FAILED_PRECONDITION with no details.
    assert_eq!(
        refusal(&CallableFailure::Status {
            status: 400,
            retry_after_seconds: None,
            body: Some(json!({
                "error": {
                    "status": "FAILED_PRECONDITION",
                    "message": "Stripe credit pack checkout unavailable"
                }
            })),
        }),
        RegistryRefusal::Unavailable
    );
    assert_eq!(
        refusal(&CallableFailure::Status {
            status: 401,
            retry_after_seconds: None,
            body: Some(json!({ "error": { "status": "UNAUTHENTICATED" } })),
        }),
        RegistryRefusal::Unauthenticated
    );
    assert_eq!(
        refusal(&CallableFailure::Status {
            status: 403,
            retry_after_seconds: None,
            body: Some(json!({ "error": { "status": "PERMISSION_DENIED" } })),
        }),
        RegistryRefusal::Unavailable
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

#[test]
fn decodes_and_validates_record_warm_bootstrap_phases_wire_contract() {
    use crate::account_mesh::cloud_provisioning::{
        RecordWarmBootstrapPhasesBody, RecordWarmBootstrapPhasesReply, WireWarmBootstrapPhase,
        DOMAIN_CLOUD_WARM_PHASES, ROUTE_CLOUD_WARM_PHASES,
    };

    let raw: Value = serde_json::from_str(FIXTURES_JSON).unwrap();
    assert_eq!(
        raw["routes"]["record_warm_bootstrap_phases"].as_str(),
        Some(ROUTE_CLOUD_WARM_PHASES)
    );
    assert_eq!(
        raw["domains"]["record_warm_bootstrap_phases"].as_str(),
        Some(DOMAIN_CLOUD_WARM_PHASES)
    );

    let reply: RecordWarmBootstrapPhasesReply =
        serde_json::from_value(raw["replies"]["record_warm_bootstrap_phases"].clone()).unwrap();
    assert_eq!(reply.machine_id, "cm_fixture0000000000000001");
    assert_eq!(reply.last_create_phases.peer_ready, Some(1_795_700_032));
    assert_eq!(reply.last_create_phases.seed_started, Some(1_795_700_033));
    assert_eq!(
        reply.last_create_phases.warm_console_ready,
        Some(1_795_700_039)
    );
    assert_eq!(reply.warm_bootstrap_delta_secs, Some(6.0));

    // Request body rejects client-supplied phase timestamps (unknown fields).
    assert!(
        serde_json::from_value::<RecordWarmBootstrapPhasesBody>(json!({
            "schema_version": 1,
            "controller_installation_id": "desk-ctrl-1",
            "machine_id": "cm_fixture0000000000000001",
            "phase": "seed_started",
            "request_id": "oaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaE",
            "timestamp": 1_795_700_033u64,
            "seed_started": 1_795_700_033u64
        }))
        .is_err()
    );

    let body = RecordWarmBootstrapPhasesBody {
        schema_version: 1,
        controller_installation_id: "desk-ctrl-1".into(),
        machine_id: "cm_fixture0000000000000001".into(),
        phase: WireWarmBootstrapPhase::WarmConsoleReady,
        request_id: "oaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaE".into(),
        timestamp: 1_795_700_039,
    };
    assert_eq!(
        serde_json::to_value(&body).unwrap()["phase"],
        json!("warm_console_ready")
    );
}
