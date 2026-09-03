use super::*;

#[test]
fn ordinary_close_preserves_team_but_explicit_close_removes_it() {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    mux.close_tab(&orchestrator).unwrap();
    mux.close_tab(&planner).unwrap();
    mux.close_tab(&worker).unwrap();
    assert!(mux.get_team(&team_id).is_some());
    let events = crate::api::events::EventBroadcaster::new();
    close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::Human,
        1,
    )
    .unwrap();
    assert!(mux.get_team(&team_id).is_none());
}

#[test]
fn close_team_unknown_and_repeat_are_structured_errors() {
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    let events = crate::api::events::EventBroadcaster::new();
    assert!(close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        "team_missing",
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap_err()
    .contains("not found"));
    close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap();
    assert!(close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap_err()
    .contains("not found"));
}

/// rel#63: a Team that lost every member (e.g. each left individually) must
/// still be closeable. The old member-authority check required the caller's
/// tab to currently BE a team member — a precondition a zero-member Team can
/// never satisfy again, deadlocking the close forever. The caller's tab stays
/// alive here (unlike a closed-tab caller, which fails for the unrelated
/// "caller tab not found" reason) — it is simply no longer on the team.
#[test]
fn zero_member_team_closes_and_cleans_up_crons() {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    for tab_id in [&orchestrator, &planner, &worker] {
        mux.get_tab_mut(tab_id).unwrap().team_id = None;
    }
    assert!(
        mux.team_member_tab_ids(&team_id).is_empty(),
        "fixture must produce a genuinely zero-member Team"
    );
    assert!(mux.get_team(&team_id).is_some());

    let events = crate::api::events::EventBroadcaster::new();
    let (closed, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .expect("a zero-member Team must be closeable by a caller no longer on it");

    assert_eq!(closed.id, team_id);
    assert!(mux.get_team(&team_id).is_none());
    assert!(cleanup.member_tab_ids.is_empty());
    // The orchestrator tab itself was never a team member at close time, so it
    // is untouched by the close (it is not team cleanup's to remove).
    assert!(mux.get_tab(&orchestrator).is_some());
}

#[test]
fn close_team_removes_chat_backend_member_and_registry() {
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    mux.get_tab_mut(&worker).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: "chat-backend".into(),
        session_id: "session-1".into(),
    };
    let events = crate::api::events::EventBroadcaster::new();

    let (_, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap();

    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_tab(&worker).is_none());
    assert_eq!(
        cleanup.chat_backings,
        vec![("chat-backend".into(), "session-1".into())]
    );
}
