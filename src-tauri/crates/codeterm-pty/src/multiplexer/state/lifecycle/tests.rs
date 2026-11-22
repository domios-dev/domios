use super::*;

#[test]
fn lifecycle_patch_preserves_live_runtime_edits_and_reverts_only_placement() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.clear_tab_role(&tab).unwrap();
    let before = mux.begin_team_close_transaction();
    let mut after = mux.begin_team_close_transaction();
    after.state_mut().set_orchestrator_role(&tab).unwrap();
    let patch = LifecyclePatch::new(before, after, ReadSet::default());
    mux.get_tab_mut(&tab).unwrap().description = Some("concurrent description".into());
    patch.apply(&mut mux).unwrap();
    assert!(mux.get_tab(&tab).unwrap().anchors_team());
    mux.get_tab_mut(&tab).unwrap().agent_session_model = Some("observed model".into());
    patch.revert(&mut mux).unwrap();
    let current = mux.get_tab(&tab).unwrap();
    assert_eq!(current.role, None);
    assert_eq!(
        current.description.as_deref(),
        Some("concurrent description")
    );
    assert_eq!(
        current.agent_session_model.as_deref(),
        Some("observed model")
    );
}

#[test]
fn lifecycle_patch_refuses_changed_identity_and_does_not_overwrite_newer_placement_on_undo() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.clear_tab_role(&tab).unwrap();
    let before = mux.begin_team_close_transaction();
    let mut after = mux.begin_team_close_transaction();
    after.state_mut().set_orchestrator_role(&tab).unwrap();
    let patch = LifecyclePatch::new(before, after, ReadSet::default());
    mux.get_tab_mut(&tab).unwrap().agent_session_id = Some("new launch".into());
    assert!(patch.apply(&mut mux).unwrap_err().contains("conflict"));
    assert_eq!(mux.get_tab(&tab).unwrap().role, None);
    mux.get_tab_mut(&tab).unwrap().agent_session_id = None;
    patch.apply(&mut mux).unwrap();
    mux.get_tab_mut(&tab).unwrap().can_spawn = false;
    assert!(patch.revert(&mut mux).unwrap_err().contains("conflict"));
    assert!(mux.get_tab(&tab).unwrap().anchors_team());
    assert!(!mux.get_tab(&tab).unwrap().can_spawn);
}

fn stage_worker(mux: &MultiplexerState, worker: &str, parent: &str) -> LifecyclePatch {
    let before = mux.begin_team_close_transaction();
    let mut after = mux.begin_team_close_transaction();
    after
        .state_mut()
        .set_tab_role(worker, AgentRole::Worker, Some(parent))
        .unwrap();
    let mut reads = ReadSet::default();
    reads.tab(parent);
    LifecyclePatch::new(before, after, reads)
}

#[test]
fn independent_worker_placements_merge_and_revert_in_the_same_workspace() {
    let (mut mux, parent_a) = MultiplexerState::new();
    let ws = mux.find_workspace_for_tab(&parent_a).unwrap();
    let parent_b = mux.create_tab(&ws).unwrap();
    let worker_a = mux.create_tab(&ws).unwrap();
    let worker_b = mux.create_tab(&ws).unwrap();
    let a = stage_worker(&mux, &worker_a, &parent_a);
    let b = stage_worker(&mux, &worker_b, &parent_b);
    a.apply(&mut mux).unwrap();
    b.apply(&mut mux).unwrap();
    let first = mux.get_tab(&worker_a).unwrap().team_id.clone().unwrap();
    let second = mux.get_tab(&worker_b).unwrap().team_id.clone().unwrap();
    assert_eq!(mux.team_order, [first.clone(), second.clone()]);
    a.revert(&mut mux).unwrap();
    assert_eq!(mux.team_order, [second.clone()]);
    assert!(mux.get_team(&first).is_none());
    assert!(mux.get_team(&second).is_some());
    assert_eq!(
        mux.get_tab(&worker_b).unwrap().parent_tab_id.as_deref(),
        Some(parent_b.as_str())
    );
    b.revert(&mut mux).unwrap();
    assert!(mux.team_order.is_empty());
    assert!(mux.get_tab(&worker_b).unwrap().role.is_none());
}

#[test]
fn sibling_close_commutes_with_staged_placement_in_either_order() {
    for close_first in [true, false] {
        let (mut mux, parent) = MultiplexerState::new();
        let ws = mux.find_workspace_for_tab(&parent).unwrap();
        let closing = mux.create_tab(&ws).unwrap();
        let worker = mux.create_tab(&ws).unwrap();
        let patch = stage_worker(&mux, &worker, &parent);
        let reservation = mux.reserve_tab_close(&closing).unwrap();
        if close_first {
            mux.close_reserved_tab_with_reason(
                reservation,
                crate::multiplexer::types::TabCloseReason::User,
            )
            .unwrap();
            patch.apply(&mut mux).unwrap();
        } else {
            patch.apply(&mut mux).unwrap();
            mux.close_reserved_tab_with_reason(
                reservation,
                crate::multiplexer::types::TabCloseReason::User,
            )
            .unwrap();
        }
        assert!(mux.get_tab(&closing).is_none());
        assert_eq!(
            mux.get_tab(&worker).unwrap().parent_tab_id.as_deref(),
            Some(parent.as_str())
        );
        patch.revert(&mut mux).unwrap();
        assert!(mux.get_tab(&worker).unwrap().role.is_none());
        assert!(mux.get_tab(&closing).is_none());
        assert!(!mux.get_workspace(&ws).unwrap().tabs.contains(&closing));
    }
}

#[test]
fn scope_reservations_and_newer_team_writes_refuse_before_any_placement_write() {
    let (mut mux, parent) = MultiplexerState::new();
    let ws = mux.find_workspace_for_tab(&parent).unwrap();
    let worker = mux.create_tab(&ws).unwrap();
    let patch = stage_worker(&mux, &worker, &parent);
    let reservation = mux.reserve_tab_close(&parent).unwrap();
    assert_eq!(
        patch.apply(&mut mux),
        Err(LIFECYCLE_CONTEXT_CONFLICT.into())
    );
    mux.abort_tab_close(&reservation);
    let reservation = mux.reserve_workspace_close(&ws).unwrap();
    assert_eq!(
        patch.apply(&mut mux),
        Err(LIFECYCLE_CONTEXT_CONFLICT.into())
    );
    mux.abort_workspace_close(&reservation);
    assert_eq!(mux.team_count(), 0);
    assert_eq!(mux.get_tab(&worker).unwrap().role, None);
    patch.apply(&mut mux).unwrap();
    let team = mux.get_tab(&worker).unwrap().team_id.clone().unwrap();
    mux.rename_team(&team, "new owner edit".into()).unwrap();
    assert!(patch.revert(&mut mux).is_err());
    assert_eq!(
        mux.get_tab(&parent).unwrap().team_id.as_deref(),
        Some(team.as_str())
    );
    assert_eq!(mux.get_tab(&worker).unwrap().role, Some(AgentRole::Worker));
    assert_eq!(
        mux.get_team(&team).unwrap().name.as_deref(),
        Some("new owner edit")
    );
}

#[test]
fn removing_and_restoring_a_team_keeps_concurrent_insertions_in_order() {
    use codeterm_types::team::Team;
    let (mut mux, tab) = MultiplexerState::new();
    let ws = mux.find_workspace_for_tab(&tab).unwrap();
    let keep = Team::new(&ws, 1);
    let removed = Team::new(&ws, 2);
    let concurrent = Team::new(&ws, 3);
    mux.upsert_team(keep.clone());
    mux.upsert_team(removed.clone());
    let before = mux.begin_team_close_transaction();
    let mut after = mux.begin_team_close_transaction();
    after.state_mut().remove_team(&removed.id).unwrap();
    let patch = LifecyclePatch::new(before, after, ReadSet::default());
    mux.upsert_team(concurrent.clone());
    patch.apply(&mut mux).unwrap();
    assert_eq!(mux.list_teams(), [keep.clone(), concurrent.clone()]);
    patch.revert(&mut mux).unwrap();
    assert_eq!(mux.list_teams(), [keep, removed, concurrent]);
}
