use super::*;
use codeterm_service_core::service::remote_team::{member_key, RemoteTeamMember};

#[tokio::test]
async fn remote_preflight_failure_retains_the_team_and_every_local_member() {
    let (mux, team, orchestrator, planner, worker) = team_members();
    let workspace = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let root = tempfile::tempdir().unwrap();
    let instance = crate::test_fixture::open_instance_with_mux(root.path(), mux);
    crate::test_fixture::install_team_close_ports(&instance, root.path());
    let fixture = &instance.fixture;
    let member = RemoteTeamMember {
        peer_id: "offline-peer".into(),
        tab_id: "remote-worker".into(),
        session_id: "remote-session".into(),
        title: "Remote".into(),
        provider: "provider".into(),
        orchestrator_tab_id: orchestrator.clone(),
        team_id: team.clone(),
        workspace_id: workspace,
        operation_id: "transfer".into(),
        finalized: false,
    };
    fixture
        .service
        .remote_teams
        .update(|data| {
            data.members
                .insert(member_key(&member.peer_id, &member.tab_id), member);
            Ok(())
        })
        .unwrap();
    let error = fixture
        .service
        .close_team(&team, TeamMutationCaller::Human, true)
        .await
        .unwrap_err();
    assert!(error.contains("remote Team preflight failed"), "{error}");
    let mux = fixture.service.mux.read();
    assert!(mux.get_team(&team).is_some());
    for id in [&orchestrator, &planner, &worker] {
        assert!(mux.get_tab(id).is_some());
    }
    assert!(
        !fixture
            .service
            .remote_teams
            .members_for_team(&team)
            .unwrap()[0]
            .finalized
    );
    drop(mux);
    codeterm_service_core::persistence_test_support::close(instance);
}
