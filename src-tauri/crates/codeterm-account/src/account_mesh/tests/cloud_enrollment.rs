use super::support::{Issuer, EPOCH, INSTALLATION, PROJECT, START};
use crate::account_mesh::{account_id, cloud_enrollment::*, key::jcs_sha256_hex, InstallationKey};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

#[tokio::test]
async fn expired_cloud_receipt_restores_only_evidence_for_same_slot_renewal() {
    let mut h = super::support::Harness::new().await;
    let key = InstallationKey::generate();
    *h.keys.0.lock().unwrap() = Some(key.to_secret());
    let issuer = Issuer::new();
    let account = account_id("owner");
    let now = START + 3600;
    h.clock.set(now);
    let sign = |slot, time| {
        issuer.sign(&Issuer::membership_payload(
            "owner",
            &key.key_id(),
            1,
            slot,
            time,
        ))
    };
    let old = sign(1, START);
    let mut row = super::support::row(INSTALLATION, 1, None);
    row.key_id = key.key_id();
    let registry = issuer.sign(&json!({"schema_version":1,"iss":"domios-mesh-registry",
        "aud":{"project":PROJECT,"env":"test"},"policy_epoch":EPOCH,"account_id":account,
        "revision":1,"issued_at":START,"members":[row],"revoked_installations":[]}));
    assert!(h
        .mesh
        .publish_cloud_membership(&account, &old, &registry)
        .await
        .is_err());
    let mut forged = old.clone();
    forged.signature = URL_SAFE_NO_PAD.encode([0; 64]);
    assert!(h
        .mesh
        .restore_cloud_membership(&account, &forged, &registry)
        .await
        .is_err());
    assert!(h
        .mesh
        .restore_cloud_membership(&account, &sign(2, START), &registry)
        .await
        .is_err());
    h.restart().await;
    h.mesh
        .restore_cloud_membership(&account, &old, &registry)
        .await
        .unwrap();
    let state = h.state();
    assert!(state.current_membership(now).is_none());
    let previous = &state.membership.as_ref().unwrap().verified;
    let known = state.signed_registry.as_ref().unwrap();
    let trust = issuer.trust();
    assert!(verify_cloud_renewal(&old, &registry, &trust, previous, known, now).is_none());
    assert!(verify_cloud_renewal(&sign(2, now), &registry, &trust, previous, known, now).is_none());
    let replacement = sign(1, now);
    assert!(verify_cloud_renewal(&replacement, &registry, &trust, previous, known, now).is_some());
    h.mesh
        .publish_cloud_membership(&account, &replacement, &registry)
        .await
        .unwrap();
    assert!(h.state().current_membership(now).is_some());
}

#[tokio::test]
async fn cloud_publication_keeps_newer_revocation_on_apply_and_restore() {
    let mut harness = super::support::Harness::new().await;
    let key = InstallationKey::generate();
    *harness.keys.0.lock().unwrap() = Some(key.to_secret());
    let issuer = Issuer::new();
    let account = account_id("owner");
    let membership = issuer.sign(&Issuer::membership_payload(
        "owner",
        &key.key_id(),
        1,
        1,
        START,
    ));
    let snapshot = |revision: u64, active: bool| {
        let mut row = super::support::row(INSTALLATION, revision, None);
        row.key_id = key.key_id();
        row.active = active;
        let revoked = if active { vec![] } else { vec![INSTALLATION] };
        issuer.sign(&json!({"schema_version":1, "iss":"domios-mesh-registry",
            "aud":{"project":PROJECT, "env":"test"}, "policy_epoch":EPOCH, "account_id":account,
            "revision":revision, "issued_at":START, "members":[row], "revoked_installations":revoked}))
    };
    let issued = snapshot(1, true);
    harness
        .mesh
        .publish_cloud_membership(&account, &membership, &issued)
        .await
        .unwrap();
    assert!(harness.state().current_membership(START).is_some());
    let state = harness.state();
    let current = &state.current_membership(START).unwrap().verified;
    let known = state.signed_registry.as_ref().unwrap();
    let trust = issuer.trust();
    let renewed = issuer.sign(&Issuer::membership_payload(
        "owner",
        &key.key_id(),
        1,
        1,
        START + 1,
    ));
    let next = snapshot(2, true);
    let verify = |membership, registry| {
        verify_cloud_renewal(membership, registry, &trust, current, known, START + 1)
    };
    assert!(verify(&renewed, &next).is_some());
    let other_slot = issuer.sign(&Issuer::membership_payload(
        "owner",
        &key.key_id(),
        1,
        2,
        START + 1,
    ));
    assert!(verify(&other_slot, &next).is_none());
    assert!(verify(&renewed, &snapshot(2, false)).is_none());
    harness
        .mesh
        .publish_cloud_membership(&account, &renewed, &next)
        .await
        .unwrap();
    harness.restart().await;
    harness
        .mesh
        .publish_cloud_membership(&account, &renewed, &next)
        .await
        .unwrap();
    assert!(harness.state().current_membership(START).is_some());
    assert!(harness
        .mesh
        .receive_registry_snapshot(&snapshot(3, false))
        .await
        .unwrap());
    for restored in [false, true] {
        if restored {
            harness.restart().await;
        }
        assert!(harness
            .mesh
            .publish_cloud_membership(&account, &membership, &issued)
            .await
            .is_err());
        let state = harness.state();
        assert!(state.current_membership(START).is_none());
        assert!(state.revoked_installations.contains(INSTALLATION));
        assert_eq!(state.signed_registry.as_ref().unwrap().revision, 3);
    }
}

#[test]
fn apply_requires_matching_controller_local_identity_session_and_both_issuer_signatures() {
    let issuer = Issuer::new();
    let trust = issuer.trust();
    let cloud = InstallationKey::generate();
    let controller = InstallationKey::generate();
    let key_id = cloud.key_id();
    let remote = controller.public_bytes();
    let nonce = [3; 32];
    let exporter = [6; 32];
    let local = Discovery {
        cloud_installation_id: INSTALLATION.into(),
        cloud_installation_key: URL_SAFE_NO_PAD.encode(cloud.public_bytes()),
        mesh_instance_id: "mesh-vps".into(),
        mesh_public_key: URL_SAFE_NO_PAD.encode([2; 32]),
    };
    let expected = ExpectedApply {
        local: &local,
        installation_key_id: &key_id,
        controller: &remote,
        nonce: &nonce,
        channel_binding: &exporter,
    };
    let body = EnrollmentBody {
        schema_version: 1,
        controller_installation_id: "home".into(),
        cloud_installation_id: INSTALLATION.into(),
        cloud_installation_key: local.cloud_installation_key.clone(),
        mesh_instance_id: local.mesh_instance_id.clone(),
        mesh_public_key: local.mesh_public_key.clone(),
        bootstrap_nonce_sha256: nonce_commitment(&nonce),
        channel_binding: URL_SAFE_NO_PAD.encode(exporter),
        provider: "timeweb".into(),
        size: "small".into(),
        request_id: URL_SAFE_NO_PAD.encode([4; 32]),
        timestamp: START,
        provider_machine_id: None,
    };
    let account = account_id("owner");
    let membership = issuer.sign(&Issuer::membership_payload("owner", &key_id, 1, 1, START));
    let mut slot = super::support::row(INSTALLATION, 1, None);
    slot.key_id = key_id.clone();
    let signed_registry = issuer.sign(&json!({"schema_version":1,"iss":"domios-mesh-registry",
        "aud":{"project":PROJECT,"env":"test"},"policy_epoch":EPOCH,"account_id":account,
        "revision":1,"issued_at":START,"members":[slot],"revoked_installations":[]}));
    let binding=issuer.sign(&json!({"schema_version":1,"iss":"domios-cloud-enrollment","aud":{"project":PROJECT,"env":"test"},
        "policy_epoch":EPOCH,"account_id":account,"controller_key":URL_SAFE_NO_PAD.encode(remote),"body":body,
        "membership_sha256":jcs_sha256_hex(&membership),"signed_registry_sha256":jcs_sha256_hex(&signed_registry),
        "issued_at":START,"expires_at":START+300}));
    let msg = enrollment_message(PROJECT, &account, &body).unwrap();
    let mut apply = Apply {
        request: EnrollmentRequest {
            body,
            controller_signature: controller.sign_cloud_enrollment(&msg),
        },
        reply: EnrollmentReply {
            membership,
            binding,
            signed_registry,
        },
    };
    assert!(verify_apply(&apply, &expected, &trust, START).is_some());
    assert!(verify_apply(
        &apply,
        &ExpectedApply {
            channel_binding: &[8; 32],
            ..expected
        },
        &trust,
        START
    )
    .is_none());
    assert!(verify_apply(
        &apply,
        &ExpectedApply {
            nonce: &[7; 32],
            ..expected
        },
        &trust,
        START
    )
    .is_none());
    assert!(verify_apply(&apply, &expected, &trust, START + 300).is_none());
    let original = apply.clone();
    apply.reply.signed_registry.signature = URL_SAFE_NO_PAD.encode([0; 64]);
    assert!(verify_apply(&apply, &expected, &trust, START).is_none());
    apply = original.clone();
    apply.request.controller_signature = cloud.sign_cloud_enrollment(&msg);
    assert!(verify_apply(&apply, &expected, &trust, START).is_none());
    apply = original.clone();
    apply.reply.membership = issuer.sign(&Issuer::membership_payload(
        "another-owner",
        &key_id,
        1,
        1,
        START,
    ));
    assert!(verify_apply(&apply, &expected, &trust, START).is_none());
    apply = original;
    apply.reply.binding.signature = URL_SAFE_NO_PAD.encode([0; 64]);
    assert!(verify_apply(&apply, &expected, &trust, START).is_none());
}
