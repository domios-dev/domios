use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn unresolved_written_payload_stays_uncertain_without_holding_the_owner() {
    let (fixture, tab) = queue_fixture(true, None);
    let accepted = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-retained");
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let owner = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(&tab);
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            owner.revoked.store(true, Ordering::Release);
            Ok(())
        }),
    );
    mark_ready(&fixture, &tab);

    assert!(matches!(
        fixture.service.drain_user_owner(&tab),
        Ok(super::super::UserDrain::Idle)
    ));

    let records = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, super::super::UserSendState::Uncertain);
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .original(&tab, &accepted.send_id)
            .unwrap(),
        FIRST_PAYLOAD
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(!fixture
        .service
        .chat_send_ledger
        .user_confirmed(&accepted.send_id));
}
