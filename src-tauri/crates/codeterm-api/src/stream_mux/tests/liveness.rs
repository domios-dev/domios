use super::*;
use futures_util::FutureExt;

struct Held {
    release: Option<std::sync::mpsc::Sender<()>>,
    holder: Option<std::thread::JoinHandle<()>>,
}
impl Held {
    async fn new(
        hold: impl FnOnce(tokio::sync::oneshot::Sender<()>, std::sync::mpsc::Receiver<()>)
            + Send
            + 'static,
    ) -> Self {
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, receive) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || hold(entered, receive));
        ready.await.unwrap();
        Self {
            release: Some(release),
            holder: Some(holder),
        }
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        self.release.take().unwrap().send(()).ok();
        self.holder.take().unwrap().join().unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn held_stream_mux_sessions_do_not_block_rest_worker() {
    let (_fixture, state) = fixture();
    let held = Held::new(|entered, release| {
        let _guard = SESSIONS.lock();
        entered.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(5)).unwrap();
    })
    .await;
    let session = format!("held-{}", uuid::Uuid::new_v4().simple());
    let mut opening = Box::pin(stream_mux_open(
        headers(),
        Query(MuxSessionQuery {
            session: session.clone(),
        }),
    ));
    assert!(opening.as_mut().now_or_never().is_none());
    crate::rest_liveness_tests::assert_health_list_send_respond(state).await;
    assert!(opening.as_mut().now_or_never().is_none());
    drop(held);
    let mut body = opening.await.unwrap().into_body().into_data_stream();
    assert_eq!(body.next().await.unwrap().unwrap(), ": mux\n\n");
    let removed = SESSIONS.lock().get(&session).unwrap().removed.clone();
    drop(body);
    tokio::time::timeout(Duration::from_secs(2), removed.notified())
        .await
        .unwrap();
    assert!(!SESSIONS.lock().contains_key(&session));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn held_stream_mux_scheduler_does_not_block_rest_worker_and_frame_is_conserved() {
    let (_fixture, state) = fixture();
    let (scheduler, mut receiver) = Scheduler::new();
    assert!(scheduler.register("held".into(), Traffic::Interactive, false));
    let blocked = scheduler.clone();
    let held = Held::new(move |entered, release| {
        blocked.with_model_held_for_test(|| {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(5)).unwrap();
        });
    })
    .await;
    let mut sending = Box::pin(scheduler.send(
        "held",
        Bytes::from_static(b"opaque-frame"),
        scheduler::Kind::Data,
        0,
    ));
    assert!(sending.as_mut().now_or_never().is_none());
    crate::rest_liveness_tests::assert_health_list_send_respond(state).await;
    assert!(sending.as_mut().now_or_never().is_none());
    drop(held);
    sending.await.unwrap();
    assert_eq!(receiver.recv().await.unwrap(), "opaque-frame");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn held_stream_mux_entry_does_not_block_rest_worker_and_cancel_settles() {
    let (_fixture, state) = fixture();
    let (session, body) = open().await;
    let reservation = reserve(
        &session,
        "held-entry",
        &headers(),
        Traffic::Interactive,
        true,
    )
    .await
    .unwrap();
    let entry = reservation.entry.clone();
    let held = Held::new(move |entered, release| {
        let _guard = entry.task.lock();
        entered.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(5)).unwrap();
    })
    .await;
    let mut cancellation = Box::pin(cancel(session.clone(), "held-entry".into(), headers()));
    assert!(cancellation.as_mut().now_or_never().is_none());
    crate::rest_liveness_tests::assert_health_list_send_respond(state).await;
    drop(held);
    assert_eq!(cancellation.await.unwrap().0["closed"], true);
    assert!(reservation.entry.detached.load(Ordering::Acquire));
    drop(reservation);
    drop(body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn cancelled_contended_receive_conserves_the_queued_frame() {
    let (scheduler, mut receiver) = Scheduler::new();
    assert!(scheduler.register("conserved".into(), Traffic::Interactive, false));
    scheduler
        .send(
            "conserved",
            Bytes::from_static(b"conserved-frame"),
            scheduler::Kind::Data,
            0,
        )
        .await
        .unwrap();
    let blocked = scheduler.clone();
    let held = Held::new(move |entered, release| {
        blocked.with_model_held_for_test(|| {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(5)).unwrap();
        });
    })
    .await;
    let mut receiving = Box::pin(receiver.recv());
    assert!(receiving.as_mut().now_or_never().is_none());
    drop(receiving);
    drop(held);
    tokio::time::timeout(
        Duration::from_secs(1),
        scheduler.contended_operation_finished_for_test(),
    )
    .await
    .expect("cancelled receive's blocking rendezvous must settle");
    let frame = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("cancelled receive must not consume its frame")
        .unwrap();
    assert_eq!(frame, "conserved-frame");
    scheduler.close();
    assert_eq!(receiver.recv().await, None);
}
