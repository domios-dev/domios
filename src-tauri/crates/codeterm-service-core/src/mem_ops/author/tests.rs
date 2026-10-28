use super::*;
use crate::{
    multiplexer::{state::MultiplexerState, types::NeighborAuthority},
    service::test_fixture::ServiceFixture,
};

#[test]
fn authenticated_tab_authority_precedes_worker_and_owner_identity() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let friend = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&friend).unwrap().neighbor_authority = Some(NeighborAuthority {
        uid: "friend-uid".into(),
        friendship_id: "friendship".into(),
        root_tab_id: None,
    });
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    *svc.ext::<OwnerMemoryAuthor>().0.write() = Some("owner-uid".into());
    assert_eq!(
        mem_author_for_caller(svc, Some(&owner)),
        MemAuthor::owner(Some("owner-uid".into()))
    );
    assert_eq!(
        mem_author_for_caller(svc, Some(&friend)),
        MemAuthor::neighbor("friend-uid".into(), "friendship".into())
    );
    assert_eq!(
        mem_author_for_caller(svc, None),
        MemAuthor::owner(Some("owner-uid".into()))
    );
}

#[test]
fn extraction_dreamer_and_calibration_tabs_are_system_authors() {
    for intent in ["extract", "dreaming:rem", "calibration"] {
        let (mut mux, tab) = MultiplexerState::new();
        mux.get_tab_mut(&tab).unwrap().worker_id = Some("worker-id".into());
        let fixture = ServiceFixture::new(mux);
        fixture.service.workers.register(serde_json::from_value(serde_json::json!({
            "worker_id":"worker-id", "tab_id":tab, "intent":intent, "spawned_by":"system", "spawned_at":0,
            "status":"writing", "touched_nodes":[], "color":"test"
        })).unwrap());
        assert_eq!(
            mem_author_for_caller(&fixture.service, Some(&tab)),
            MemAuthor::system(intent)
        );
    }
}
