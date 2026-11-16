use codeterm_service_core::service::agent_runtime::engine::{self, sync_fixture};

#[test]
fn runtime_publication_does_not_refreeze_after_unpause() {
    crate::register_neighbors();
    let (fixture, tab, _peer, _cleanup) = sync_fixture::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    let friendship = "a".repeat(64);
    {
        let mut mux = fixture.service.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.neighbor_authority = Some(
            codeterm_service_core::multiplexer::types::NeighborAuthority {
                uid: "owner".into(),
                friendship_id: friendship.clone(),
                root_tab_id: None,
            },
        );
    }
    assert!(fixture
        .service
        .record_neighbor_pause_state("owner", &friendship, true, None));
    assert!(fixture.service.mux.read().get_tab(&tab).unwrap().frozen);
    let (entered, release) = engine::hold_publication_for_test(&tab);
    let svc = fixture.service.clone();
    let publishing_tab = tab.clone();
    let publication = std::thread::spawn(move || engine::publish(&svc, &publishing_tab, &session));
    entered
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    assert!(fixture
        .service
        .record_neighbor_pause_state("owner", &friendship, false, None));
    assert!(!fixture.service.mux.read().get_tab(&tab).unwrap().frozen);
    release.send(()).unwrap();
    publication.join().unwrap();
    assert!(!fixture.service.mux.read().get_tab(&tab).unwrap().frozen);
}
