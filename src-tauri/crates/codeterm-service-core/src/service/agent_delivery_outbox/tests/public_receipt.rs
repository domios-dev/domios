use super::*;
use futures_util::FutureExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn restored_send_waits_for_its_single_write_without_blocking_the_async_worker() {
    let (fixture, tab) = fixture(Some("saved-session"), false);
    let writes = Arc::new(fixture.record_agent_deliveries(&tab));
    let delivery = fixture
        .service
        .deliver_agent_payload(&tab, payload(), "visible")
        .unwrap();
    assert!(matches!(&delivery, Delivery::Pending { .. }));
    let retained = fixture
        .service
        .public_delivery_receipt(&tab, delivery.clone());
    let Delivery::Pending { delivery_id } = &retained else {
        unreachable!()
    };
    let mut waiting = Box::pin(fixture.service.await_restored_send_receipt(
        &tab,
        delivery.clone(),
        Duration::from_secs(5),
    ));
    assert!(waiting.as_mut().now_or_never().is_none());
    assert_eq!(tokio::spawn(async { 42 }).await.unwrap(), 42);
    assert!(writes.texts.lock().is_empty());
    authenticate(&fixture, &tab);
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap(),
        Delivery::Pty { .. }
    ));
    let finished = writes.clone();
    tokio::task::spawn_blocking(move || finished.wait_for_flush())
        .await
        .unwrap();
    assert_eq!(*writes.texts.lock(), ["opaque held payload"]);
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert_eq!(
        fixture
            .service
            .inspect_retained_send(delivery_id)
            .unwrap()
            .unwrap()
            .state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );
    // A second observer starting after publication proves the subscribe/inspection race is covered.
    assert!(matches!(
        fixture
            .service
            .await_restored_send_receipt(&tab, delivery, Duration::from_secs(5))
            .await,
        Delivery::Pty { .. }
    ));
    assert_eq!(*writes.texts.lock(), ["opaque held payload"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn cancelled_receipt_waiter_never_cancels_or_replays_the_retained_send() {
    let (fixture, tab) = fixture(Some("saved-session"), false);
    let writes = Arc::new(fixture.record_agent_deliveries(&tab));
    let delivery = fixture
        .service
        .deliver_agent_payload(&tab, payload(), "visible")
        .unwrap();
    let mut waiting = Box::pin(fixture.service.await_restored_send_receipt(
        &tab,
        delivery.clone(),
        Duration::from_secs(5),
    ));
    assert!(waiting.as_mut().now_or_never().is_none());
    drop(waiting);
    assert!(fixture.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
    authenticate(&fixture, &tab);
    assert!(matches!(
        fixture
            .service
            .await_restored_send_receipt(&tab, delivery, Duration::from_secs(5))
            .await,
        Delivery::Pty { .. }
    ));
    let finished = writes.clone();
    tokio::task::spawn_blocking(move || finished.wait_for_flush())
        .await
        .unwrap();
    assert_eq!(*writes.texts.lock(), ["opaque held payload"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn held_receipt_deadline_does_not_claim_a_write_or_settle_the_original() {
    let (fixture, tab) = fixture(Some("saved-session"), false);
    let writes = Arc::new(fixture.record_agent_deliveries(&tab));
    let delivery = fixture
        .service
        .deliver_agent_payload(&tab, payload(), "visible")
        .unwrap();
    let observed = fixture
        .service
        .await_restored_send_receipt(&tab, delivery, Duration::ZERO)
        .await;
    let Delivery::Pending { delivery_id } = observed else {
        panic!("deadline cannot claim a write");
    };
    assert!(fixture.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
    fixture
        .service
        .cancel_retained_agent_send(&delivery_id)
        .unwrap();
}
