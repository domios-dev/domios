use super::*;

#[test]
fn process_split_contract_runtime_and_team_port_share_service_state() {
    let (mux, team, _, _, _) = team_members();
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    let _storage = fixture.install_seeded_storage_desk();
    let mut events = fixture.service.events.subscribe();
    let service = fixture.service.clone();
    let renamed = team.clone();
    codeterm_service_core::async_runtime::block_on(async {
        codeterm_service_core::async_runtime::spawn_blocking(move || {
            service.rename_team(&renamed, "portable-team".into(), TeamMutationCaller::Human)
        })
        .await
        .unwrap()
        .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.frame.event_type, "mux-state-changed");
    });
    assert_eq!(
        fixture.service.mux.read().get_team(&team).unwrap().name,
        "portable-team"
    );
    codeterm_service_core::service::session_persist::stop_worker(&fixture.service).unwrap();
}
