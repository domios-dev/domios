use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::persistence_test_support::{close, ServiceInstance};

#[test]
fn team_rename_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&tab_id)
        .unwrap()
        .team_id;
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    crate::commands::mux_commands::mux_rename_team_service_core(
        &instance.fixture.service,
        team_id.clone(),
        "renamed in service".into(),
        None,
    )
    .expect("rename Team through the command service boundary");

    let restarted = instance.restart(root.path());
    assert_eq!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_team(&team_id)
            .and_then(|team| team.name.as_deref()),
        Some("renamed in service")
    );
    close(restarted);
}
