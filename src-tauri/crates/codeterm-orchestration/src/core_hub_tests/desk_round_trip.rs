use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::persistence_test_support::close;
use codeterm_service_core::service::TeamMutationCaller;

use crate::TeamExt;

#[test]
fn team_create_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("identity-provider".into());
        tab.cwd = Some(root.path().to_string_lossy().into_owned());
    }
    mux.set_orchestrator_role(&tab_id).unwrap();
    let instance = crate::test_fixture::open_instance_with_mux(root.path(), mux);
    crate::deliver_orchestrator_onboarding(&instance.fixture.service, &tab_id);
    let team_id = instance
        .fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.team_id.clone())
        .expect("onboarding creates a Team through ServiceContext");

    let restarted = instance.restart(root.path());
    assert!(restarted
        .fixture
        .service
        .mux
        .read()
        .get_team(&team_id)
        .is_some());
    close(restarted);
}

#[test]
fn team_close_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&tab_id)
        .unwrap()
        .team_id;
    let instance = crate::test_fixture::open_instance_with_mux(root.path(), mux);
    tauri::async_runtime::block_on(instance.fixture.service.close_team(
        &team_id,
        TeamMutationCaller::Human,
        true,
    ))
    .expect("close Team through ServiceContext");

    let restarted = instance.restart(root.path());
    assert!(restarted
        .fixture
        .service
        .mux
        .read()
        .get_team(&team_id)
        .is_none());
    close(restarted);
}
