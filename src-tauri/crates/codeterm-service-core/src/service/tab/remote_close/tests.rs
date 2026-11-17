use super::*;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::RemoteOwnership;

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
        let result = fixture
            .service
            .close_tab_from_authenticated_mesh_caller(peer, agent, &target, false);
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
        .close_tab_from_authenticated_mesh_caller("creator-peer", None, &target, true,)
        .unwrap_err()
        .starts_with(REMOTE_TAB_CLOSE_REFUSAL_PREFIX));
}
