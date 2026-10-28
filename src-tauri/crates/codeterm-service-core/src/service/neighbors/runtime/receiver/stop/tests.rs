use super::*;

#[test]
fn pending_global_stop_fences_racing_startup_until_retirement() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    service.set_neighbor_friend_receiver_for_test("friend",Some(true),1);
    assert!(service.neighbor_friend_receiving_here("friend"));
    service.queue_neighbor_stop_local();
    service.set_neighbor_friend_receiver_for_test("friend",Some(true),1);
    assert!(!service.neighbor_friend_receiving_here("friend"));
    *service.extensions.get_or_init(Pending::default).0.lock() = None;
    assert!(service.neighbor_friend_receiving_here("friend"));
}
