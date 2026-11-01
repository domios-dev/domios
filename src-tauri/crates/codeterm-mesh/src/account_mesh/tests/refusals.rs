use super::support::*;
use crate::account_mesh::gate::{self, BlockReason, TrafficDecision};
use crate::account_mesh::ledger::{LinkAdmission, LinkRecord};
use crate::account_mesh::{AccountMeshState, AccountPresence};

const LOCKED: TrafficDecision = TrafficDecision::Blocked(BlockReason::PlanLimitLocked);

fn signed_in(locked: bool, enrolled: bool) -> AccountMeshState {
    AccountMeshState {
        account_epoch: 1,
        presence: AccountPresence::SignedIn {
            account_id: ACCOUNT.into(),
        },
        membership: enrolled.then(|| super::support::membership(ACCOUNT, "self", START + 60)),
        revoked_installations: Default::default(),
        unavailable_installations: Default::default(),
        plan_limit_locked: locked,
    }
}

fn account_link() -> LinkRecord {
    LinkRecord {
        link_nonce: None,
        admission: LinkAdmission::Account {
            account_id: ACCOUNT.into(),
            installation_id: "peer".into(),
            public_key: "peer-key".into(),
        },
        remote_membership: Some(membership(ACCOUNT, "peer", START + 60)),
    }
}

fn decide(record: &LinkRecord, state: &AccountMeshState) -> TrafficDecision {
    gate::decide(Some(record), "peer-key", state, START)
}

#[test]
fn account_mesh_machine_over_limit_blocks_account_edge_with_plan_limit_code() {
    assert_eq!(decide(&account_link(), &signed_in(true, false)), LOCKED);
    assert_eq!(
        decide(&account_link(), &signed_in(false, false)),
        TrafficDecision::Blocked(BlockReason::LocalMembershipExpired),
        "an expired lease without a limit refusal keeps its own reason"
    );
}

#[test]
fn account_mesh_paid_snapshot_opens_account_edge() {
    let paid = signed_in(false, true);
    assert!(decide(&account_link(), &paid).is_open());
}

#[test]
fn plan_limit_locked_wire_literal_is_the_entitlements_code() {
    let wire = serde_json::to_value(crate::account_mesh::ports::AdmissionRefusal::PlanLimitLocked)
        .unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"reason": codeterm_entitlements::PLAN_LIMIT_LOCKED})
    );
    assert_eq!(
        crate::account_mesh::ports::AdmissionRefusal::PlanLimitLocked.code(),
        codeterm_entitlements::PLAN_LIMIT_LOCKED
    );
}
