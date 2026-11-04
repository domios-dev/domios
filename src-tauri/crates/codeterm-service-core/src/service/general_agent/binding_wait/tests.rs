use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn binding_event_during_async_verdict_is_not_lost() {
    let bound = Arc::new(AtomicBool::new(false));
    let observed = bound.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(async move {
        let mut first = Some((started_tx, finish_rx));
        wait_for_session_binding_async(Instant::now() + Duration::from_secs(1), || {
            let first = first.take();
            let observed = observed.clone();
            async move {
                if let Some((started, finish)) = first {
                    let _ = started.send(());
                    finish.await.unwrap();
                    None
                } else {
                    observed.load(Ordering::SeqCst).then_some("bound")
                }
            }
        })
        .await
    });
    started_rx.await.unwrap();
    bound.store(true, Ordering::SeqCst);
    notify_binding_state_changed();
    finish_tx.send(()).unwrap();
    assert_eq!(waiter.await.unwrap(), Some("bound"));
}

#[test]
fn binding_notification_releases_waiter_without_polling() {
    let bound = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&bound);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let value = wait_for_session_binding(Instant::now() + Duration::from_secs(1), || {
            ready_tx.send(()).ok();
            observed.load(Ordering::SeqCst).then_some("session-id")
        });
        done_tx.send(value).unwrap();
    });

    ready_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("waiter must inspect the binding before waiting");
    bound.store(true, Ordering::SeqCst);
    notify_binding_state_changed();

    assert_eq!(
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("binding event must release the waiter"),
        Some("session-id")
    );
    waiter.join().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn async_binding_wait_leaves_the_runtime_available_for_other_requests() {
    use std::sync::atomic::Ordering;

    let bound = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = std::sync::Arc::clone(&bound);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(async move {
        let mut ready_tx = Some(ready_tx);
        wait_for_session_binding_async(Instant::now() + std::time::Duration::from_secs(1), || {
            let ready_tx = ready_tx.take();
            let value = observed.load(Ordering::SeqCst).then_some("bound");
            async move {
                if let Some(ready_tx) = ready_tx {
                    let _ = ready_tx.send(());
                }
                value
            }
        })
        .await
    });

    ready_rx
        .await
        .expect("waiter must inspect state before waiting");
    let other_request = tokio::spawn(async { "completed" });
    assert_eq!(other_request.await.unwrap(), "completed");
    bound.store(true, Ordering::SeqCst);
    notify_binding_state_changed();
    assert_eq!(waiter.await.unwrap(), Some("bound"));
}
