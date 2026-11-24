use super::*;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::RemoteOwnership;

fn mesh_grant(fixture: &ServiceFixture, current: bool) -> MeshCloseGrant {
    MeshCloseGrant::new(
        fixture.service.ext::<crate::MeshStateHandle>().clone(),
        move |_| current,
    )
}

fn fixture() -> (ServiceFixture, String) {
    let (mut mux, initial) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&initial).unwrap();
    let target = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&target).unwrap().ownership = Some(RemoteOwnership {
        peer_id: "creator-peer".into(),
        peer_name: "Creator".into(),
        controller_tab_id: Some("creator-agent".into()),
        controller_title: "Creator".into(),
        team: None,
    });
    (ServiceFixture::new(mux), target)
}

#[test]
fn remote_close_allows_only_the_stored_creator_agent_or_its_machine() {
    for (peer, agent, allowed) in [
        ("creator-peer", Some("creator-agent"), true),
        ("creator-peer", None, true),
        ("creator-peer", Some("foreign-agent"), false),
        ("foreign-peer", Some("creator-agent"), false),
        ("foreign-peer", None, false),
        ("", None, false),
    ] {
        let (fixture, target) = fixture();
        let result = fixture.service.close_tab_from_authenticated_mesh_caller(
            peer,
            agent,
            &target,
            false,
            &mesh_grant(&fixture, true),
        );
        assert_eq!(result.is_ok(), allowed, "{peer} {agent:?}: {result:?}");
        assert_eq!(
            fixture.service.mux.read().get_tab(&target).is_none(),
            allowed
        );
    }
}

#[test]
fn remote_close_refuses_released_ownership_and_rechecks_before_commit() {
    let (fixture, target) = fixture();
    let result = fixture.service.close_tab_with_teardown(
        &target,
        TabCloseReason::User,
        |mux, tab| authorize(mux, tab, "creator-peer", Some("creator-agent")),
        || {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&target)
                .unwrap()
                .ownership = None;
            Ok(())
        },
        |mux, tab| authorize(mux, tab, "creator-peer", Some("creator-agent")),
    );
    assert!(result
        .unwrap_err()
        .starts_with(REMOTE_TAB_CLOSE_REFUSAL_PREFIX));
    assert!(fixture.service.mux.read().get_tab(&target).is_some());
    assert!(fixture
        .service
        .close_tab_from_authenticated_mesh_caller(
            "creator-peer",
            None,
            &target,
            true,
            &mesh_grant(&fixture, true)
        )
        .unwrap_err()
        .starts_with(REMOTE_TAB_CLOSE_REFUSAL_PREFIX));
}

#[test]
fn creator_close_refuses_revoked_grant_before_reservation() {
    let (fixture, target) = fixture();
    let error = fixture
        .service
        .close_tab_from_authenticated_mesh_caller(
            "creator-peer",
            Some("creator-agent"),
            &target,
            true,
            &mesh_grant(&fixture, false),
        )
        .unwrap_err();
    assert!(error.contains("Peer authorization changed"));
    let mut mux = fixture.service.mux.write();
    assert!(mux.get_tab(&target).is_some());
    let reservation = mux.reserve_tab_close(&target).unwrap();
    mux.abort_tab_close(&reservation);
}
