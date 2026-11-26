use super::support::{Issuer, EPOCH, INSTALLATION, PROJECT, START};
use crate::account_mesh::{account_id, cloud_enrollment::*, key::jcs_sha256_hex, InstallationKey};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

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
        cloud_mode: None,
        request_id: URL_SAFE_NO_PAD.encode([4; 32]),
        timestamp: START,
    };
    let account = account_id("owner");
    let membership = issuer.sign(&Issuer::membership_payload("owner", &key_id, 1, 1, START));
    let binding=issuer.sign(&json!({"schema_version":1,"iss":"domios-cloud-enrollment","aud":{"project":PROJECT,"env":"test"},
        "policy_epoch":EPOCH,"account_id":account,"controller_key":URL_SAFE_NO_PAD.encode(remote),"body":body,
        "membership_sha256":jcs_sha256_hex(&membership),"issued_at":START,"expires_at":START+300}));
    let msg = enrollment_message(PROJECT, &account, &body).unwrap();
    let mut apply = Apply {
        request: EnrollmentRequest {
            body,
            controller_signature: controller.sign_cloud_enrollment(&msg),
        },
        reply: EnrollmentReply {
            membership,
            binding,
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
