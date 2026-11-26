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
        provisioning_message, CloudProvisioningRefusal, CreateCloudMachineRequest,
        DeleteMachineReply, ManagedCloudMachineActionRequest, ProvisionedMachineReply,
        SleepMachineReply, WireCloudSleepMode, CLOUD_INGRESS_PORT, DOMAIN_CLOUD_CREATE,
        DOMAIN_CLOUD_DELETE, DOMAIN_CLOUD_SLEEP, DOMAIN_CLOUD_WAKE, ROUTE_CLOUD_CREATE,
        ROUTE_CLOUD_DELETE, ROUTE_CLOUD_SLEEP, ROUTE_CLOUD_WAKE,
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
    wake: ProvisionedMachineReply,
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

    // Verify all 6 reply fixtures decode with expected endpoints, sleep_mode, and rates.
    assert_eq!(
        fixtures.replies.create.validated_endpoint(),
        Some(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT
        ))
    );
    assert_eq!(
        fixtures.replies.create.sleep_mode,
        WireCloudSleepMode::DeleteRecreate
    );
    assert_eq!(fixtures.replies.create.rate.basis, "managed");
    assert_eq!(fixtures.replies.create.rate.credits_per_hour, 1.0);

    // delete_recreate wake returns a new IPv4 on the same machine_id.
    assert_eq!(
        fixtures.replies.wake.machine_id,
        fixtures.replies.create.machine_id
    );
    assert_eq!(
        fixtures.replies.wake.validated_endpoint(),
        Some(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 42),
            CLOUD_INGRESS_PORT
        ))
    );
    assert_eq!(fixtures.replies.sleep.state, "asleep");
    assert_eq!(
        fixtures.replies.sleep.sleep_mode,
        WireCloudSleepMode::DeleteRecreate
    );
    assert_eq!(fixtures.replies.delete.state, "deleted");
    assert!(fixtures.replies.enroll.signed_registry.is_some());
    assert!(fixtures.replies.renew.signed_registry.is_some());
}

#[test]
fn decodes_all_ledger_cloud_provisioning_refusal_fixtures() {
    let fixtures: FixtureFile =
        serde_json::from_str(FIXTURES_JSON).expect("decode shared/cloud-provisioning fixtures");
    assert_eq!(fixtures.refusals.len(), 20);

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
