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
        plan_limit_locked: locked,
    }
}

fn legacy() -> LinkRecord {
    LinkRecord {
        admission: LinkAdmission::Legacy {
            public_key: "peer-key".into(),
        },
        remote_membership: None,
    }
}

fn adopted() -> LinkRecord {
    LinkRecord {
        admission: LinkAdmission::Adopted {
            account_id: ACCOUNT.into(),
            installation_id: "peer".into(),
            public_key: "peer-key".into(),
        },
        remote_membership: Some(membership(ACCOUNT, "peer", START + 60)),
    }
}

fn decide(record: &LinkRecord, state: &AccountMeshState) -> TrafficDecision {
    gate::decide(Some(record), "peer-key", Some(ACCOUNT), state, START)
}

#[test]
fn account_mesh_machine_over_limit_blocks_legacy_edge_with_plan_limit_code() {
    let decision = decide(&legacy(), &signed_in(true, false));
    assert_eq!(decision, LOCKED);
    assert_eq!(BlockReason::PlanLimitLocked.code(), "plan_limit_locked");
}

#[test]
fn account_mesh_machine_over_limit_blocks_adopted_edge_with_plan_limit_code() {
    assert_eq!(decide(&adopted(), &signed_in(true, false)), LOCKED);
    assert_eq!(
        decide(&adopted(), &signed_in(false, false)),
        TrafficDecision::Blocked(BlockReason::LocalMembershipExpired),
        "an expired lease without a limit refusal keeps its own reason"
    );
}

#[test]
fn account_mesh_paid_snapshot_opens_both_edges() {
    let paid = signed_in(false, true);
    assert!(decide(&legacy(), &paid).is_open());
    assert!(decide(&adopted(), &paid).is_open());
}

#[test]
fn account_mesh_dormant_gate_keeps_legacy_open() {
    let never = AccountMeshState::default();
    assert!(decide(&legacy(), &never).is_open());
    assert!(decide(&legacy(), &signed_in(false, false)).is_open());
}
