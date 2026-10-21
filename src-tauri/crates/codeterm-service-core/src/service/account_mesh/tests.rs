use super::authority::{admission_refusal, membership_refusal};
use super::*;
use codeterm_account::account_mesh::{
    Cleanup, MembershipRefusal, SignedMembership, VerifiedMembership,
};
use codeterm_mesh::account_mesh as mesh;
use serde_json::json;

fn signed_in(epoch: u64) -> LifecycleState {
    LifecycleState {
        account_epoch: epoch,
        presence: Presence::SignedIn {
            account_id: "a".repeat(64),
        },
        owner: Some("a".repeat(64)),
        membership: Some(SignedMembership {
            token: "{\"payload\":\"p\"}".into(),
            verified: VerifiedMembership {
                account_id: "a".repeat(64),
                installation_id: "installation-a".into(),
                key_id: "k".repeat(64),
                key_generation: 1,
                slot_revision: 2,
                issued_at: 100,
                expires_at: 3700,
            },
        }),
        revoked_installations: ["installation-c".to_owned()].into(),
        ..LifecycleState::default()
    }
}

#[test]
fn account_mesh_state_reaches_the_gate_with_epoch_presence_membership_and_revocations() {
    let state = mesh_state(&signed_in(3));
    assert_eq!(state.account_epoch, 3);
    assert_eq!(state.signed_in_account(), Some("a".repeat(64).as_str()));
    let membership = state.current_membership(100).expect("current");
    assert_eq!(membership.verified.key_id, "k".repeat(64));
    assert_eq!(membership.verified.slot_revision, 2);
    assert!(
        state.current_membership(3700).is_none(),
        "expiry is exclusive"
    );
    assert!(state.revoked_installations.contains("installation-c"));

    let signed_out = LifecycleState {
        presence: Presence::SignedOut,
        membership: None,
        ..signed_in(4)
    };
    let state = mesh_state(&signed_out);
    assert_eq!(state.presence, mesh::AccountPresence::SignedOut);
    assert!(state.membership.is_none());
    assert_eq!(
        mesh_state(&LifecycleState::default()).presence,
        mesh::AccountPresence::NeverSignedIn
    );
}

#[test]
fn account_mesh_only_the_machine_limit_reaches_the_gate_as_mesh_machines() {
    let limit = admission_refusal(AccountMeshRefusal::MachineLimit {
        limit: 2,
        active_count: 2,
    });
    assert_eq!(
        serde_json::to_value(&limit).unwrap(),
        json!({"reason": "mesh.machines", "limit": 2, "active_count": 2})
    );
    for refusal in [
        AccountMeshRefusal::Invalid,
        AccountMeshRefusal::Unauthenticated,
        AccountMeshRefusal::AppCheck,
        AccountMeshRefusal::Forbidden,
        AccountMeshRefusal::Conflict,
        AccountMeshRefusal::RateLimited {
            retry_after_seconds: 30,
        },
        AccountMeshRefusal::AuthorityUnavailable,
        AccountMeshRefusal::Unavailable,
        AccountMeshRefusal::Offline,
        AccountMeshRefusal::VerificationUnavailable,
        AccountMeshRefusal::Revoked,
        AccountMeshRefusal::SignedOut,
        AccountMeshRefusal::Cancelled,
    ] {
        let mapped = admission_refusal(refusal);
        assert!(
            !matches!(mapped, mesh::AdmissionRefusal::MachineLimit { .. }),
            "{refusal:?}"
        );
    }
    assert_eq!(
        admission_refusal(AccountMeshRefusal::SignedOut),
        mesh::AdmissionRefusal::SignedOut
    );
    assert_eq!(
        admission_refusal(AccountMeshRefusal::Conflict),
        mesh::AdmissionRefusal::handshake("conflict")
    );
    assert_eq!(
        membership_refusal(MembershipRefusal::Expired),
        mesh::AdmissionRefusal::Revoked
    );
    assert_eq!(
        membership_refusal(MembershipRefusal::Signature),
        mesh::AdmissionRefusal::handshake("membership-signature")
    );
    assert_eq!(
        membership_refusal(MembershipRefusal::NotYetValid),
        mesh::AdmissionRefusal::handshake("membership-not_yet_valid"),
        "a clock behind the issuer is not a revocation"
    );
}

#[test]
fn account_mesh_snapshot_and_outcomes_match_the_desktop_client() {
    let mut state = signed_in(2);
    state.cleanup = Cleanup::Pending;
    let snapshot = AccountMeshSnapshot {
        view: state.view(),
        pairs: Vec::new(),
    };
    let value = serde_json::to_value(&snapshot).unwrap();
    for key in [
        "binding",
        "traffic",
        "cleanup",
        "self",
        "registry",
        "registry_failure",
        "pairs",
        "limit_notice",
    ] {
        assert!(value.get(key).is_some(), "{key}");
    }
    assert_eq!(value["binding"], "current");
    assert_eq!(value["traffic"], "open");
    assert_eq!(value["cleanup"], "pending");
    assert!(
        value.get("membership").is_none(),
        "membership tokens never leave the daemon"
    );

    let refused = AccountMeshOutcome::<()>::of(
        Err(AccountMeshRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        }),
        Some("r".repeat(64)),
    );
    assert_eq!(
        serde_json::to_value(&refused).unwrap(),
        json!({"ok": false, "request_id": "r".repeat(64),
            "failure": {"reason": "mesh.machines", "limit": 2, "active_count": 2}})
    );
    let done = AccountMeshOutcome::of(Ok(()), None);
    assert_eq!(
        serde_json::to_value(&done).unwrap(),
        json!({"ok": true, "result": null})
    );
}

#[test]
fn account_mesh_start_failure_never_opens_past_a_persisted_sign_out() {
    let signed_out = AccountMeshRecord {
        signed_out: true,
        ..AccountMeshRecord::default()
    };
    assert_eq!(
        fallback_mesh_state(Ok(signed_out)).presence,
        mesh::AccountPresence::SignedOut
    );
    assert_eq!(
        fallback_mesh_state(Err(std::io::Error::other("corrupt"))).presence,
        mesh::AccountPresence::SignedOut,
        "an unreadable record fails closed"
    );
    assert_eq!(
        fallback_mesh_state(Ok(AccountMeshRecord::default())).presence,
        mesh::AccountPresence::NeverSignedIn,
        "an upgrade that never signed out keeps legacy pairs"
    );
}

#[test]
fn account_mesh_unchanged_lifecycle_pushes_do_not_wake_the_gate() {
    let sender = watch::channel(mesh::AccountMeshState::default()).0;
    let mut receiver = sender.subscribe();
    assert!(publish_mesh_state(&sender, mesh_state(&signed_in(1))));
    assert!(receiver.has_changed().unwrap());
    drop(receiver.borrow_and_update());
    assert!(!publish_mesh_state(&sender, mesh_state(&signed_in(1))));
    assert!(
        !receiver.has_changed().unwrap(),
        "same projection, no wakeup"
    );
    assert!(publish_mesh_state(&sender, mesh_state(&signed_in(2))));
}

#[test]
fn account_mesh_not_before_skew_matches_across_account_and_mesh() {
    assert_eq!(
        codeterm_account::account_mesh::NOT_BEFORE_SKEW_SECONDS,
        codeterm_mesh::account_mesh::ports::NOT_BEFORE_SKEW_SECONDS
    );
}
