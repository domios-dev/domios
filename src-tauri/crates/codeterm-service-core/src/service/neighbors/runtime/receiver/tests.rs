use super::*;

#[tokio::test]
async fn receiver_fence_requires_current_enabled_installation_and_revision() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    assert_eq!(
        service.require_neighbor_receiver(None),
        Err(Error::NotReceiver)
    );
    let mut status = ReceiverStatus {
        receiver_revision: 8,
        receiver_installation_id: Some("machine".into()),
        receiver_key_id: None,
        receiver_machine_label: Some("Desk".into()),
        receiving_enabled: true,
        is_me: true,
        legacy_active: false,
    };
    for (is_me, enabled, revision, allowed) in [
        (true, true, 8, true),
        (false, true, 8, false),
        (true, false, 8, false),
        (true, true, 7, false),
    ] {
        status.is_me = is_me;
        status.receiving_enabled = enabled;
        *service.neighbor_cell().receiver.lock() = Some(status.clone());
        assert_eq!(
            service.require_neighbor_receiver(Some(revision)).is_ok(),
            allowed
        );
    }
    assert_eq!(
        ServiceContext::receiver_view(&status),
        json!({"receiving_here":true,"receiver_machine_label":"Desk","receiving_enabled":true,"revision":8})
    );
    assert!(serde_json::from_value::<NeighborRuntimeCommand>(
        json!({"operation":"enroll","takeover_confirmed":true})
    )
    .is_err());
}
