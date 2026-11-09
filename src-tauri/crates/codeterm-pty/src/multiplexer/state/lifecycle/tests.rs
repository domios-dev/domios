use super::*;

#[test]
fn lifecycle_patch_preserves_live_runtime_edits_and_reverts_only_placement() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.clear_tab_role(&tab).unwrap();
    let before = mux.begin_team_close_transaction();
    let mut after = mux.begin_team_close_transaction();
    after.state_mut().set_orchestrator_role(&tab).unwrap();
    let patch = LifecyclePatch::new(before, after);
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
    let patch = LifecyclePatch::new(before, after);
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
