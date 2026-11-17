use super::*;

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
async fn newer_recovery_generation_cancels_an_in_flight_attempt_and_stale_events() {
    let owner = RecoveryOwner::default();
    let old = owner.begin();
    let (_exit, exit) = watch::channel(false);
    let current = owner.begin();
    old.cancelled(exit).await;
    assert!(!old.current());
    assert!(current.current());
}

#[tokio::test]
async fn exit_or_window_close_cancels_waiting_recovery_before_any_new_attempt() {
    for quit in [false, true] {
        let owner = RecoveryOwner::default();
        let lease = owner.begin();
        let (exit_sender, exit) = watch::channel(false);
        if quit {
            exit_sender.send_replace(true);
        } else {
            owner.cancel();
        }
        let mut attempts = 0;
        tokio::select! {
            biased;
            _ = lease.cancelled(exit) => {},
            _ = std::future::ready(()) => { attempts += 1; },
        }
        assert_eq!(attempts, 0);
    }
}

#[test]
fn retryable_failures_get_two_automatic_attempts_then_a_manual_page() {
    for code in [
        FailureCode::RootAckDeadline,
        FailureCode::NoServerAttachment,
        FailureCode::NativeDisconnect,
        FailureCode::StartupStreamFailed,
        FailureCode::NavigationFailed,
        FailureCode::InitialDocumentFailed,
    ] {
        let mut incident = Incident::default();
        assert_eq!(incident.failure(code), Decision::Recover);
        assert_eq!(incident.next_attempt(true), Some(1));
        assert_eq!(incident.failure(code), Decision::Join);
        assert_eq!(incident.next_attempt(true), Some(2));
        assert_eq!(incident.next_attempt(true), None);
        assert_eq!(incident.failure(code), Decision::ShowFailure);
        assert_eq!(incident.attempt, 2);
    }
}

#[test]
fn unsafe_refusal_stays_manual_even_with_a_ready_event() {
    for code in [
        FailureCode::StartupRefused,
        FailureCode::ServerAttachmentRefused,
        FailureCode::ClientRetryRefused,
    ] {
        let mut incident = Incident::default();
        assert_eq!(incident.failure(code), Decision::ShowFailure);
        assert_eq!(incident.attempt, 0);
        let mut running = Incident::default();
        assert_eq!(
            running.failure(FailureCode::NoServerAttachment),
            Decision::Recover
        );
        assert_eq!(running.next_attempt(true), Some(1));
        assert_eq!(running.next_attempt(code.recoverable()), None);
        assert_eq!(running.attempt, 1);
    }
}

#[tokio::test]
async fn transient_attach_failure_recovers_on_later_actual_ready_without_a_click() {
    use std::future::{poll_fn, Future};
    use std::task::Poll;
    let (sender, mut events) = watch::channel(RootOutcome::Pending);
    let mut waiting = Box::pin(root_outcome(&mut events));
    assert!(poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
        .await
        .is_pending());
    sender.send_replace(RootOutcome::Ready);
    assert_eq!(waiting.await, Ok(()));
}

#[tokio::test]
async fn failed_root_event_preserves_the_exact_branch_for_the_next_attempt() {
    let failure =
        FrontendFailure::new(FailureCode::RootAckDeadline, "root acknowledgement missing");
    let (_sender, mut events) = watch::channel(RootOutcome::Failed(failure.clone()));
    assert_eq!(root_outcome(&mut events).await, Err(failure));
}

#[test]
fn only_a_new_navigation_can_complete_an_active_incident() {
    let mut incident = Incident {
        after_navigation: 8,
        ..Default::default()
    };
    assert!(!incident.accepts_root(9));
    assert_eq!(
        incident.failure(FailureCode::RootAckDeadline),
        Decision::Recover
    );
    assert!(!incident.accepts_root(7));
    assert!(!incident.accepts_root(8));
    assert!(incident.accepts_root(9));
}
