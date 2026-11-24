use super::*;
use crate::service::test_fixture::ServiceFixture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[test]
fn owner_tab_reservation_refuses_a_workspace_move_and_a_stale_mesh_grant() {
    let (mut mux, initial) = MultiplexerState::new();
    let original = mux.find_workspace_for_tab(&initial).unwrap();
    let tab = mux.create_tab(&original).unwrap();
    let destination = mux.create_workspace("Destination".into(), None).id;
    let fixture = ServiceFixture::new(mux);
    let workspace = OwnerTabCloseScope::Workspace(original.clone());
    workspace.check(&fixture.service.mux.read(), &tab).unwrap();
    fixture
        .service
        .mux
        .write()
        .move_tab(&tab, &original, &destination)
        .unwrap();
    assert!(workspace
        .reserve(&mut fixture.service.mux.write(), &tab)
        .is_err());

    let current = Arc::new(AtomicBool::new(true));
    let check = current.clone();
    let mesh = fixture.service.ext::<crate::MeshStateHandle>().clone();
    let scope = OwnerTabCloseScope::Mesh(MeshCloseGrant::new(mesh, move |_| {
        check.load(Ordering::Relaxed)
    }));
    scope.check(&fixture.service.mux.read(), &tab).unwrap();
    current.store(false, Ordering::Relaxed);
    let mut mux = fixture.service.mux.write();
    assert!(scope
        .reserve(&mut mux, &tab)
        .err()
        .unwrap()
        .contains("Peer authorization changed"));
    let valid = OwnerTabCloseScope::Workspace(destination);
    valid.reserve(&mut mux, &tab).unwrap();
}

#[test]
fn team_close_snapshot_refuses_members_added_after_workspace_confirmation() {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = "team";
    mux.get_tab_mut(&tab).unwrap().team_id = Some(team.into());
    let targets = TeamCloseSnapshot::capture(&mux, team, BTreeMap::new());
    targets.validate(&mux, team).unwrap();
    let added = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&added).unwrap().team_id = Some(team.into());
    assert!(targets.validate(&mux, team).is_err());
}
