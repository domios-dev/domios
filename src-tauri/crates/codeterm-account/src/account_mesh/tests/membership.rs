use super::support::*;
use crate::account_mesh::wire::*;
use crate::account_mesh::*;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use codeterm_entitlements::DetachedGrant;
use ed25519_dalek::{Signature, Signer, SigningKey};
use serde_json::{json, Value};

const PACKET: &str = include_str!("account-mesh-packet.json");
const UID: &str = "user-1";
const KEY_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn packet() -> Value {
    serde_json::from_str(PACKET).unwrap()
}

fn grant(payload: &Value) -> DetachedGrant {
    Issuer::new().sign(payload)
}

fn valid_payload() -> Value {
    Issuer::membership_payload(UID, KEY_ID, 1, 2, START)
}

fn expect(account: &str) -> MembershipExpectation<'_> {
    MembershipExpectation {
        account_id: account,
        installation_id: Some(INSTALLATION),
        key_id: Some(KEY_ID),
        key_generation: Some(1),
        slot_revision: Some(2),
    }
}

fn verify(payload: &Value, now: u64) -> Result<VerifiedMembership, MembershipRefusal> {
    let account = account_id(UID);
    verify_membership(
        &grant(payload),
        &Issuer::new().trust(),
        &expect(&account),
        now,
    )
}

fn with(mut payload: Value, field: &str, value: Value) -> Value {
    payload[field] = value;
    payload
}

#[test]
fn account_mesh_membership_accepts_only_its_exact_window() {
    let verified = verify(&valid_payload(), START).unwrap();
    assert_eq!(verified.account_id, account_id(UID));
    assert_eq!(verified.refresh_after(), START + 2700);
    assert!(verify(&valid_payload(), START + 3599).is_ok());
    assert_eq!(
        verify(&valid_payload(), START + 3600),
        Err(MembershipRefusal::Expired)
    );
    assert!(
        verify(&valid_payload(), START - NOT_BEFORE_SKEW_SECONDS).is_ok(),
        "a desktop clock behind the issuer still accepts a fresh membership"
    );
    assert!(verified.is_current(START - NOT_BEFORE_SKEW_SECONDS));
    assert_eq!(
        verify(&valid_payload(), START - NOT_BEFORE_SKEW_SECONDS - 1),
        Err(MembershipRefusal::NotYetValid)
    );
    assert!(!verified.is_current(START - NOT_BEFORE_SKEW_SECONDS - 1));
    assert!(!verified.is_current(START + 3600));
    let long = with(valid_payload(), "expires_at", json!(START + 86401));
    assert_eq!(verify(&long, START), Err(MembershipRefusal::Duration));
}

#[test]
fn account_mesh_membership_binds_issuer_audience_epoch_subject_installation_key_and_slot() {
    let cases = [
        (
            with(valid_payload(), "iss", json!("domios-entitlements")),
            MembershipRefusal::Issuer,
        ),
        (
            with(
                valid_payload(),
                "aud",
                json!({"project": "other", "env": "test"}),
            ),
            MembershipRefusal::Audience,
        ),
        (
            with(
                valid_payload(),
                "aud",
                json!({"project": PROJECT, "env": "live"}),
            ),
            MembershipRefusal::Audience,
        ),
        (
            with(valid_payload(), "policy_epoch", json!("old-epoch")),
            MembershipRefusal::Epoch,
        ),
        (
            with(valid_payload(), "sub", json!("user-2")),
            MembershipRefusal::Subject,
        ),
        (
            with(valid_payload(), "installation_id", json!("installation-x")),
            MembershipRefusal::Installation,
        ),
        (
            with(valid_payload(), "key_id", json!("2".repeat(64))),
            MembershipRefusal::Key,
        ),
        (
            with(valid_payload(), "key_generation", json!(2)),
            MembershipRefusal::Key,
        ),
        (
            with(valid_payload(), "slot_revision", json!(3)),
            MembershipRefusal::Slot,
        ),
    ];
    for (payload, refusal) in cases {
        assert_eq!(verify(&payload, START), Err(refusal), "{payload}");
    }
}

#[test]
fn account_mesh_membership_refuses_entitlement_grants_and_tampering() {
    let entitlement =
        json!({"schema_version": 3, "aud": "desktop", "uid": UID, "installation_id": INSTALLATION});
    assert_eq!(
        verify(&entitlement, START),
        Err(MembershipRefusal::Malformed)
    );
    let mut extra = valid_payload();
    extra["max_mesh_machines"] = json!(null);
    assert_eq!(verify(&extra, START), Err(MembershipRefusal::Malformed));

    let account = account_id(UID);
    let trust = Issuer::new().trust();
    let mut forged = grant(&valid_payload());
    forged.signature = URL_SAFE_NO_PAD.encode([0u8; 64]);
    assert_eq!(
        verify_membership(&forged, &trust, &expect(&account), START),
        Err(MembershipRefusal::Signature)
    );
    let mut unknown = grant(&valid_payload());
    unknown.key_id = "issuer-2".into();
    assert_eq!(
        verify_membership(&unknown, &trust, &expect(&account), START),
        Err(MembershipRefusal::UnknownKey)
    );
    let other = SigningKey::from_bytes(&[9u8; 32]);
    let text = codeterm_entitlements::jcs::encode(&valid_payload()).unwrap();
    let foreign = DetachedGrant {
        signature: URL_SAFE_NO_PAD.encode(other.sign(text.as_bytes()).to_bytes()),
        ..grant(&valid_payload())
    };
    assert_eq!(
        verify_membership(&foreign, &trust, &expect(&account), START),
        Err(MembershipRefusal::Signature)
    );
    let spaced = format!(" {text}");
    let issuer = SigningKey::from_bytes(&[7u8; 32]);
    let noncanonical = DetachedGrant {
        payload: URL_SAFE_NO_PAD.encode(spaced.as_bytes()),
        signature: URL_SAFE_NO_PAD.encode(issuer.sign(spaced.as_bytes()).to_bytes()),
        key_id: ISSUER_KEY.into(),
        alg: "Ed25519".into(),
    };
    assert_eq!(
        verify_membership(&noncanonical, &trust, &expect(&account), START),
        Err(MembershipRefusal::NonCanonical)
    );
    let token = membership_token(&grant(&valid_payload()));
    assert_eq!(
        parse_membership_token(&token).unwrap(),
        grant(&valid_payload())
    );
}

#[test]
fn account_mesh_installation_key_matches_the_server_key_id_and_survives_its_secret() {
    let key = InstallationKey::generate();
    let restored = InstallationKey::from_secret(&key.to_secret()).unwrap();
    assert_eq!(restored.key_id(), key.key_id());
    let spki = key.spki_base64();
    assert!(codeterm_entitlements::policy::parse_spki(&spki).is_some());
    let der = STANDARD.decode(&spki).unwrap();
    assert_eq!(key.key_id(), super::super::key::sha256_hex(&der));
    assert_eq!(key.key_id().len(), 64);
}

/// Pins the exact signed bytes: JCS of the domain, server context and unsigned proof.
#[test]
fn account_mesh_proof_signs_the_domain_separated_canonical_context() {
    let seed = crate::Secret::new(URL_SAFE_NO_PAD.encode([5u8; 32]));
    let key = InstallationKey::from_secret(&seed).unwrap();
    let context = ProofContext {
        project: PROJECT.into(),
        mode: "test",
        uid: UID.into(),
        installation_id: INSTALLATION.into(),
        policy_epoch: EPOCH.into(),
        method: "POST",
        endpoint: ProofEndpoint::MeshAdmission,
        body_sha256: "d".repeat(64),
    };
    let proof = key.prove(&context, 1, START, "n".into()).unwrap();
    assert_eq!(proof.schema_version, 1);
    assert_eq!(proof.signature.len(), 86);
    let expected = format!(
        "{{\"body_sha256\":\"{}\",\"domain\":\"domios.online-installation-proof.v1\",\"endpoint\":\"meshAdmission\",\"generation\":1,\"installation_id\":\"{INSTALLATION}\",\"key_id\":\"{}\",\"method\":\"POST\",\"mode\":\"test\",\"nonce\":\"n\",\"policy_epoch\":\"{EPOCH}\",\"project\":\"{PROJECT}\",\"schema_version\":1,\"timestamp\":{START},\"uid\":\"{UID}\"}}",
        "d".repeat(64),
        key.key_id()
    );
    let verifying = codeterm_entitlements::policy::parse_spki(&key.spki_base64()).unwrap();
    let signature: [u8; 64] = URL_SAFE_NO_PAD
        .decode(&proof.signature)
        .unwrap()
        .try_into()
        .unwrap();
    assert!(verifying
        .verify_strict(expected.as_bytes(), &Signature::from_bytes(&signature))
        .is_ok());
}

#[test]
fn account_mesh_possession_binds_the_enrolled_key_and_the_transcript() {
    let key = InstallationKey::generate();
    let transcript = [4u8; 32];
    let possession = key.sign_possession(&transcript);
    assert!(verify_possession(&key.key_id(), &transcript, &possession));
    assert!(!verify_possession(&key.key_id(), &[5u8; 32], &possession));
    let other = InstallationKey::generate();
    assert!(!verify_possession(
        &other.key_id(),
        &transcript,
        &possession
    ));
    let substituted = other.sign_possession(&transcript);
    assert!(!verify_possession(&key.key_id(), &transcript, &substituted));
    assert!(!verify_possession(&key.key_id(), &transcript, "garbage"));
}

#[test]
fn account_mesh_link_permit_binds_holder_peer_subject_and_transcript() {
    let holder = MeshTarget {
        installation_id: PEER.into(),
        key_id: "2".repeat(64),
        key_generation: 1,
        slot_revision: 3,
    };
    let own = MeshTarget {
        installation_id: INSTALLATION.into(),
        key_id: KEY_ID.into(),
        key_generation: 1,
        slot_revision: 2,
    };
    let transcript = [8u8; 32];
    let digest = super::super::key::sha256_hex(&transcript);
    let permit = grant(&Issuer::permit_payload(UID, &holder, &own, &digest, START));
    let account = account_id(UID);
    let expectation = LinkPermitExpectation {
        transcript: &transcript,
        holder_installation_id: PEER,
        peer_installation_id: INSTALLATION,
        account_id: &account,
        peer_target: Some(&own),
    };
    let trust = Issuer::new().trust();
    assert_eq!(
        verify_link_permit(&permit, &trust, &expectation, START + 119),
        Ok(())
    );
    assert_eq!(
        verify_link_permit(&permit, &trust, &expectation, START + 120),
        Err(MembershipRefusal::Expired)
    );
    let other_transcript = LinkPermitExpectation {
        transcript: &[9u8; 32],
        ..expectation
    };
    assert_eq!(
        verify_link_permit(&permit, &trust, &other_transcript, START),
        Err(MembershipRefusal::Installation)
    );
    let stale_slot = MeshTarget {
        slot_revision: 4,
        ..own.clone()
    };
    let moved = LinkPermitExpectation {
        peer_target: Some(&stale_slot),
        ..expectation
    };
    assert_eq!(
        verify_link_permit(&permit, &trust, &moved, START),
        Err(MembershipRefusal::Slot)
    );
    let membership = grant(&valid_payload());
    assert_eq!(
        verify_link_permit(&membership, &trust, &expectation, START),
        Err(MembershipRefusal::Malformed)
    );
}

#[test]
fn account_mesh_wire_matches_the_server_packet() {
    let packet = packet();
    assert_eq!(packet["routes"]["challenge"], ROUTE_CHALLENGE);
    assert_eq!(packet["routes"]["enrollKey"], ROUTE_ENROLL_KEY);
    assert_eq!(packet["routes"]["admission"], ROUTE_ADMISSION);
    let base = |request_id: &str, operation: MeshOperation| MeshAdmissionBody {
        schema_version: 1,
        installation_id: "desk-7f3a".into(),
        request_id: request_id.into(),
        requested_membership_seconds: None,
        operation,
    };
    let id = |n: u8| format!("{}{n}", "0".repeat(63));
    let peer = MeshTarget {
        installation_id: "laptop-91c0".into(),
        key_id: "2".repeat(64),
        key_generation: 1,
        slot_revision: 3,
    };
    let requests = [
        (
            "publish",
            base(
                &id(1),
                MeshOperation::Publish {
                    expected_metadata_revision: 0,
                    endpoint: MeshEndpoint::parse("100.64.0.7", 7685).unwrap().wire(),
                },
            ),
        ),
        (
            "publishIpv6",
            base(
                &id(2),
                MeshOperation::Publish {
                    expected_metadata_revision: 1,
                    endpoint: MeshEndpoint::parse("fd7a:115c:a1e0::7", 7685)
                        .unwrap()
                        .wire(),
                },
            ),
        ),
        (
            "withdraw",
            base(
                &id(3),
                MeshOperation::Withdraw {
                    expected_metadata_revision: 2,
                },
            ),
        ),
        (
            "link",
            base(
                &id(4),
                MeshOperation::Link {
                    expected_slot_revision: 2,
                    peer,
                    transcript_sha256: "3".repeat(64),
                },
            ),
        ),
        ("list", base(&id(5), MeshOperation::List { cursor: None })),
    ];
    for (name, request) in requests {
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            packet["requests"][name],
            "{name}"
        );
    }
    for name in ["published", "withdrawn", "page", "permit"] {
        let result: MeshResult = serde_json::from_value(packet["results"][name].clone()).unwrap();
        assert!(matches!(
            (name, &result),
            ("published", MeshResult::Published(_))
                | ("withdrawn", MeshResult::Withdrawn(_))
                | ("page", MeshResult::Page(_))
                | ("permit", MeshResult::Permit(_))
        ));
    }
    let MeshResult::Page(page): MeshResult =
        serde_json::from_value(packet["results"]["page"].clone()).unwrap()
    else {
        panic!("page");
    };
    assert_eq!(
        page.rows[0].endpoint.as_ref().unwrap().tailnet_ip,
        "100.64.0.7"
    );
    assert!(page.rows[1].endpoint.is_none());
    let challenge = ChallengeRequest {
        endpoint: ProofEndpoint::MeshAdmission,
        key_id: "1".repeat(64),
        body: packet["requests"]["publish"].clone(),
    };
    assert_eq!(
        serde_json::to_value(&challenge).unwrap(),
        packet["callableEnvelope"]["challenge"]
    );
}

#[test]
fn account_mesh_endpoint_accepts_only_canonical_tailnet_addresses() {
    let packet = packet();
    for rejected in packet["rejectedEndpoints"].as_array().unwrap() {
        assert!(
            MeshEndpoint::parse(rejected.as_str().unwrap(), 7685).is_none(),
            "{rejected}"
        );
    }
    assert!(MeshEndpoint::parse("100.64.0.7", 0).is_none());
    assert!(MeshEndpoint::parse("100.127.255.254", 7685).is_some());
    assert!(MeshEndpoint::parse("fd7a:115c:a1e0::7", 7685).is_some());
}

#[test]
fn account_mesh_refusal_details_map_exactly_and_only_mesh_machines_is_a_plan_limit() {
    let packet = packet();
    let refusals = packet["refusals"].as_object().unwrap();
    for (name, refusal) in refusals {
        let parsed = RegistryRefusal::from_details(&refusal["details"]);
        let desktop = AccountMeshRefusal::from(parsed);
        assert_eq!(
            serde_json::to_value(desktop).unwrap(),
            refusal["details"],
            "{name}"
        );
        assert_eq!(
            matches!(desktop, AccountMeshRefusal::MachineLimit { .. }),
            name == "mesh.machines"
        );
    }
    for malformed in [
        json!({"reason": "mesh.machines"}),
        json!({"reason": "mesh.machines", "limit": 0, "active_count": 2}),
        json!({"reason": "rate-limited", "retry_after_seconds": 0}),
        json!({"reason": "offline"}),
        json!({"reason": "grant"}),
        json!("mesh.machines"),
    ] {
        assert_eq!(
            RegistryRefusal::from_details(&malformed),
            RegistryRefusal::Unavailable,
            "{malformed}"
        );
    }
}

#[test]
fn account_mesh_short_membership_renewal_is_strictly_after_issuance() {
    for (seconds, refresh) in [(1, 1), (119, 60), (120, 60), (121, 61), (3600, 2700)] {
        let payload = with(valid_payload(), "expires_at", json!(START + seconds));
        let membership = verify(&payload, START).unwrap();
        assert_eq!(membership.refresh_after(), START + refresh);
        assert!(membership.is_current(START));
        assert!(!membership.is_current(START + seconds));
    }
}
