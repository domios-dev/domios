use super::*;

/// Guard for codeterm-releases#48: `transfer_team_plan_core` never reads
/// `plan.planner`, so a dead planner pointer cannot block it. No live defect.
#[test]
fn transfer_ignores_a_dead_planner_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, source_owner) = MultiplexerState::new();
    let source_team = mux
        .resolve_or_mint_team_for_parent(&source_owner)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&source_owner).unwrap();
    let destination_owner = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&destination_owner).unwrap();
    let destination_team = mux
        .resolve_or_mint_team_for_parent(&destination_owner)
        .unwrap()
        .team_id;
    let plan = store
        .create_plan_for_team(
            "Dead planner pointer transfer".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&source_owner),
            source_team,
            vec![],
            1,
        )
        .unwrap();
    store
        .set_planner_ref(
            &plan.id,
            Some(crate::plan::PlannerRef::Tab {
                tab_id: "tab_does_not_exist_in_mux".into(),
                session_id: None,
                title: None,
            }),
            2,
        )
        .unwrap();
    assert!(
        mux.get_tab("tab_does_not_exist_in_mux").is_none(),
        "the fixture's planner pointer must genuinely be dead"
    );

    let transferred = transfer_team_plan_core(
        &mut mux,
        &store,
        &plan.id,
        crate::plan::OrchestratorRef::from_tab(&destination_owner),
        "transfer despite dead planner pointer",
        TeamMutationCaller::AgentTab(source_owner.clone()),
        3,
    )
    .expect("a dead planner pointer must not block a transfer that never resolves it");

    assert_eq!(
        transferred.team_id.as_deref(),
        Some(destination_team.as_str())
    );
    assert_eq!(
        transferred.owner.tab_id.as_deref(),
        Some(destination_owner.as_str())
    );
}
