use super::*;
use serde_json::json;

#[test]
fn team_show_keeps_the_team_object_and_includes_durable_remote_members() {
    let (mut mux, orchestrator) =
        codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let team = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let member = codeterm_service_core::service::remote_team::RemoteTeamMember {
        peer_id: "peer".into(),
        tab_id: "worker".into(),
        session_id: "session".into(),
        title: "Worker".into(),
        provider: "provider".into(),
        orchestrator_tab_id: orchestrator,
        team_id: team.clone(),
        workspace_id: workspace,
        operation_id: "operation".into(),
        finalized: false,
    };
    fixture
        .service
        .remote_teams
        .update(|data| {
            data.members.insert("peer/worker".into(), member);
            Ok(())
        })
        .unwrap();
    let service = std::sync::Arc::new(fixture.service.clone());
    let result = dispatch(
        "team.show",
        &Some(json!({"team_id":team})),
        &service.mux,
        &service,
    )
    .unwrap();
    assert_eq!(result["team"]["id"], team);
    let rows = result["members"].as_array().unwrap();
    assert!(rows
        .iter()
        .any(|row| row["tab_id"] == "mesh:peer/worker" && row["session_id"] == "session"));
}

#[test]
fn team_lifecycle_advances_after_plan_approval_and_retains_closing() {
    let mut team = codeterm_types::team::Team::new("ws", 1);
    let mut plan = codeterm_service_core::plan::Plan::new(
        "Approved work".into(),
        "/project".into(),
        codeterm_service_core::plan::OrchestratorRef::from_tab("orch"),
    );
    plan.team_id = Some(team.id.clone());
    plan.mode = codeterm_service_core::plan::PlanMode::Approved;
    assert_eq!(
        lifecycle_for_team(&team, &[plan.clone()]),
        codeterm_types::team::TeamLifecycle::Active
    );
    team.lifecycle = codeterm_types::team::TeamLifecycle::Closing;
    assert_eq!(
        lifecycle_for_team(&team, &[plan]),
        codeterm_types::team::TeamLifecycle::Closing
    );
}

#[test]
fn team_members_reports_cartographer_as_a_manager_kind() {
    let (mut mux, orchestrator) =
        codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let cartographer = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &cartographer,
        codeterm_types::agent::roles::AgentRole::Manager,
        Some(codeterm_types::agent::role_kind::builtins::CARTOGRAPHER),
        Some(&orchestrator),
    )
    .unwrap();

    let members = team_members(&mux, &team_id);
    let member = members
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["tab_id"] == cartographer)
        .expect("cartographer is a Team member");
    assert_eq!(member["role"], "cartographer");
    assert_eq!(member["role_type"], "Manager");
    assert_eq!(member["role_profile"], "cartographer");
}

#[test]
fn required_caller_tab_names_the_refused_action() {
    let error = required_caller_tab(&Some(json!({})), "Team agent adoption").unwrap_err();
    assert_eq!(error.code, -32602);
    assert!(
        error.message.contains("Team agent adoption"),
        "{}",
        error.message
    );
}

#[test]
fn adopt_plan_caller_requires_nonempty_tab_id() {
    assert_eq!(
        adopt_plan_caller(&Some(json!({ "caller_tab_id": "manager" }))).unwrap(),
        "manager"
    );
    for params in [
        None,
        Some(json!({})),
        Some(json!({ "caller_tab_id": null })),
        Some(json!({ "caller_tab_id": "  " })),
    ] {
        let error = adopt_plan_caller(&params).unwrap_err();
        assert_eq!(error.code, -32602);
        assert!(error.message.contains("caller_tab_id is required"));
    }
}

#[test]
fn close_inventory_failure_has_rpc_data_and_a_readable_message() {
    let error = close_team_rpc_error(
        json!({
            "type": "close_inventory_timeout",
            "message": "capture deadline"
        })
        .to_string(),
    );
    assert_eq!(error.code, -32000);
    assert_eq!(error.message, "capture deadline");
    assert_eq!(error.data.unwrap()["type"], "close_inventory_timeout");
    let ordinary = close_team_rpc_error("permission refused".into());
    assert_eq!(ordinary.code, -32602);
    assert_eq!(ordinary.message, "permission refused");
    assert!(ordinary.data.is_none());
}
