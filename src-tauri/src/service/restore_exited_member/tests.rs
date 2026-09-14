use std::sync::Arc;

use codeterm_util::lock_hold::TimedRwLock;

use super::*;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::{AgentRole, PriorBindingNonce};

/// A Worker whose provider exited to its shell outside a shutdown, taken
/// through the real teardown and a serialized snapshot restore.
fn restored_exited_worker(bound: bool) -> (MultiplexerState, String) {
    let (mut state, _) = MultiplexerState::new();
    let workspace = state.create_workspace("restore-exited".into(), None).id;
    let tab_id = state.create_tab(&workspace).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = bound.then(|| "exited-session".to_string());
    tab.session_binding_nonce = Some("exited-nonce".into());
    tab.reported_session_binding_nonce = bound.then(|| "exited-nonce".to_string());
    tab.cwd = Some("D:/fixture".into());
    tab.role = Some(AgentRole::Worker);
    tab.team_id = Some("team-fixture".into());
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));

    let raw = serde_json::to_string(&state.to_session_snapshot()).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&serde_json::from_str(&raw).unwrap());
    (restored, tab_id)
}

#[test]
fn restore_contract_exited_member_lineage_survives_snapshot_restore() {
    let (restored, tab_id) = restored_exited_worker(true);
    let tab = restored.get_tab(&tab_id).unwrap();

    assert!(tab.restored_from_snapshot);
    assert_eq!(tab.agent_type, None);
    assert_eq!(tab.agent_session_id, None);
    assert_eq!(tab.role, Some(AgentRole::Worker));
    assert_eq!(tab.team_id.as_deref(), Some("team-fixture"));
    assert_eq!(
        tab.prior_session_binding_nonces,
        vec![PriorBindingNonce {
            session_id: Some("exited-session".into()),
            nonce: "exited-nonce".into(),
        }]
    );
    assert_eq!(
        exited_members(&restored),
        vec![(tab_id.clone(), "exited-session".to_string())]
    );
}

#[test]
fn restore_contract_exited_member_is_flagged_and_never_relaunched() {
    let (restored, tab_id) = restored_exited_worker(true);
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-exited-member", restored));

    assert!(
        crate::service::pty_io::claim_restored_pty_boot(&mux, &tab_id)
            .unwrap()
            .is_none()
    );
    let mux = mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(!tab.agent_booting);
    assert_eq!(tab.agent_type, None);
    assert_eq!(exited_member_session(tab), Some("exited-session"));
}

#[test]
fn restore_contract_only_restored_members_with_a_bound_exited_session_are_flagged() {
    let (unbound, unbound_id) = restored_exited_worker(false);
    assert_eq!(
        exited_member_session(unbound.get_tab(&unbound_id).unwrap()),
        None,
        "an exited launch that never bound a session has nothing to recover"
    );

    let (restored, tab_id) = restored_exited_worker(true);
    let flagged = restored.get_tab(&tab_id).unwrap().clone();
    let mut roleless = flagged.clone();
    roleless.role = None;
    let mut declared = flagged.clone();
    declared.agent_type = Some("fixture-provider".into());
    let mut claimed = flagged.clone();
    claimed.agent_session_id = Some("exited-session".into());
    let mut live_record = flagged.clone();
    live_record.restored_from_snapshot = false;

    for tab in [roleless, declared, claimed, live_record] {
        assert_eq!(exited_member_session(&tab), None, "{:?}", tab.id);
    }
    assert_eq!(exited_member_session(&flagged), Some("exited-session"));
}
