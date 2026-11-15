use super::*;
use futures_util::FutureExt;

async fn fill_output(scheduler: &Scheduler) -> Result<(), Push> {
    loop {
        scheduler
            .send_output("output", Bytes::from_static(b"data"), Kind::Data, 0)
            .await?;
    }
}

#[tokio::test]
async fn output_event_larger_than_queue_is_delivered_whole_to_a_slow_consumer() {
    let (scheduler, mut receiver) = Scheduler::new();
    assert!(scheduler.register("output".into(), Traffic::Bulk, true));
    assert!(scheduler.register("foreground".into(), Traffic::Interactive, false));
    let event = format!("data: {}\n\n", "x".repeat(128 * 1024));
    let mut producer = Box::pin(async {
        for chunk in event.as_bytes().chunks(8 * 1024) {
            scheduler
                .send_output("output", Bytes::copy_from_slice(chunk), Kind::Data, 0)
                .await?;
        }
        Ok::<(), Push>(())
    });
    assert!(producer.as_mut().now_or_never().is_none());
    let queued = scheduler.0.model.lock().buffered;
    assert!(queued <= model::BYTE_BUDGET);
    scheduler
        .send("foreground", Bytes::from_static(b"rpc"), Kind::Data, 0)
        .await
        .unwrap();
    assert_eq!(receiver.recv().await.unwrap(), "rpc");
    let (other, mut other_receiver) = Scheduler::new();
    assert!(other.register("other-client".into(), Traffic::Bulk, true));
    other
        .send_output(
            "other-client",
            Bytes::from_static(b"independent"),
            Kind::Data,
            0,
        )
        .await
        .unwrap();
    assert_eq!(other_receiver.recv().await.unwrap(), "independent");
    let mut received = Vec::new();
    loop {
        received.extend_from_slice(&receiver.recv().await.unwrap());
        if let Some(result) = producer.as_mut().now_or_never() {
            result.unwrap();
            break;
        }
        assert!(scheduler.0.model.lock().buffered <= model::BYTE_BUDGET);
    }
    while scheduler.0.model.lock().buffered != 0 {
        received.extend_from_slice(&receiver.recv().await.unwrap());
    }
    assert_eq!(received, event.as_bytes());
    assert_eq!(scheduler.0.model.lock().buffered, 0);
}

#[tokio::test(start_paused = true)]
async fn output_space_wait_has_a_failure_bound() {
    let (scheduler, _receiver) = Scheduler::new();
    assert!(scheduler.register("output".into(), Traffic::Bulk, true));
    let mut producer = Box::pin(fill_output(&scheduler));
    assert!(producer.as_mut().now_or_never().is_none());
    tokio::time::advance(OUTPUT_SPACE_DEADLINE).await;
    assert_eq!(producer.await, Err(Push::Overflow));
    scheduler.remove("output");
    assert_eq!(scheduler.0.model.lock().buffered, 0);
}

#[tokio::test]
async fn cancelling_output_wakes_its_space_wait_and_releases_queued_bytes() {
    let (scheduler, _receiver) = Scheduler::new();
    assert!(scheduler.register("output".into(), Traffic::Bulk, true));
    let mut producer = Box::pin(fill_output(&scheduler));
    assert!(producer.as_mut().now_or_never().is_none());
    scheduler.remove("output");
    assert_eq!(producer.await, Err(Push::Closed));
    assert_eq!(scheduler.0.model.lock().buffered, 0);
}
