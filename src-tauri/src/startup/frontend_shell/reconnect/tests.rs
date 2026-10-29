use super::*;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
async fn manual_retry_handoff_waits_for_the_cancelled_owner_to_release_its_flight() {
    let owner = RecoveryOwner::default();
    let lease = owner.begin();
    let mut completion = owner.cancel_pending().unwrap();
    assert!(!lease.current());
    assert!(!*completion.borrow_and_update());
    drop(lease);
    completion.changed().await.unwrap();
    assert!(*completion.borrow_and_update());
}

#[tokio::test]
async fn transient_attach_failure_recovers_on_later_actual_ready_without_a_click() {
    let owner = RecoveryOwner::default();
    let lease = owner.begin();
    let (_exit, exit) = watch::channel(false);
    let (ready, mut events) = mpsc::channel(2);
    let (attempted, first_attempt) = tokio::sync::oneshot::channel();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let operation = tokio::spawn(async move {
        let mut attempted = Some(attempted);
        recover(&mut events, lease.cancelled(exit), move || {
            let index = count.fetch_add(1, Ordering::SeqCst);
            let signal = attempted.take();
            async move {
                if let Some(signal) = signal {
                    let _ = signal.send(());
                }
                if index == 0 {
                    Err(AttemptFailure::Transient(
                        "attach temporarily refused".into(),
                    ))
                } else {
                    Ok("replacement-generation")
                }
            }
        })
        .await
    });
    first_attempt.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    ready.send(Availability::ServerReady).await.unwrap();
    assert!(matches!(
        operation.await.unwrap(),
        Outcome::Ready("replacement-generation")
    ));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn newer_recovery_generation_cancels_an_in_flight_attempt_and_stale_events() {
    let owner = RecoveryOwner::default();
    let old = owner.begin();
    let (_exit, exit) = watch::channel(false);
    let (ready, mut events) = mpsc::channel(2);
    let (attempted, first_attempt) = tokio::sync::oneshot::channel();
    let operation = tokio::spawn(async move {
        let mut attempted = Some(attempted);
        recover(&mut events, old.cancelled(exit), move || {
            let signal = attempted.take();
            async move {
                if let Some(signal) = signal {
                    let _ = signal.send(());
                }
                std::future::pending::<Result<(), AttemptFailure>>().await
            }
        })
        .await
    });
    first_attempt.await.unwrap();
    let current = owner.begin();
    assert!(matches!(operation.await.unwrap(), Outcome::Cancelled));
    assert!(ready.send(Availability::ServerReady).await.is_err());
    assert!(current.current());
}

#[tokio::test]
async fn exit_or_window_close_cancels_waiting_recovery_before_any_new_attempt() {
    for quit in [false, true] {
        let owner = RecoveryOwner::default();
        let lease = owner.begin();
        let (exit_sender, exit) = watch::channel(false);
        let (_ready, mut events) = mpsc::channel(2);
        if quit {
            exit_sender.send_replace(true);
        } else {
            owner.cancel();
        }
        let attempts = AtomicUsize::new(0);
        let count = &attempts;
        let result = recover(&mut events, lease.cancelled(exit), move || async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<(), AttemptFailure>(())
        })
        .await;
        assert!(matches!(result, Outcome::Cancelled));
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn unsafe_refusal_stays_manual_even_with_a_ready_event() {
    let (sender, mut events) = mpsc::channel(2);
    sender.send(Availability::ServerReady).await.unwrap();
    let attempts = AtomicUsize::new(0);
    let result = recover(&mut events, std::future::pending(), || {
        attempts.fetch_add(1, Ordering::SeqCst);
        std::future::ready(Err::<(), _>(AttemptFailure::Manual(
            "incompatible identity".into(),
        )))
    })
    .await;
    assert!(matches!(result, Outcome::Failed(reason) if reason == "incompatible identity"));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}
