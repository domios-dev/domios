use super::*;

#[test]
fn ready_dispatch_preserves_admission_order_when_wakes_arrive_reversed() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    let mut records = Vec::new();
    for (sequence, text) in [(1, "first"), (2, "second")] {
        let reservation = store
            .reserve(authority.clone(), None, || {
                (format!("{tab}-{sequence}"), sequence)
            })
            .unwrap();
        let record = store
            .admit_with_follow_up(&reservation, text, text, None, None, false)
            .unwrap();
        fixture.service.chat_send_ledger.admit_user(&record);
        records.push(record);
    }
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_owned()))
                .unwrap();
            Ok(())
        }),
    );
    fixture.service.dispatch_ready_user_send(&records[1]);
    let first_write = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    fixture.service.dispatch_ready_user_send(&records[0]);
    let second_write = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(first_write, (records[0].send_id.clone(), "first".into()));
    assert_eq!(
        second_write,
        (records[1].send_id.clone(), "\n\nsecond".into())
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}
