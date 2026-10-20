use super::super::new_restart_tracker;
use super::*;

#[test]
fn wake_cannot_pass_a_transition_until_retirement_finishes() {
    let tracker = new_restart_tracker();
    let claim = claim(&tracker, "own-tab").unwrap();
    let retirement = request_wake(&tracker, "own-tab");
    assert!(claim.wake_requested());
    assert!(request_wake(&tracker, "other-tab").is_none());
    assert!(super::claim(&tracker, "own-tab").is_err());
    std::thread::scope(|scope| {
        let (completed, completion) = std::sync::mpsc::channel();
        scope.spawn(move || {
            completed
                .send(wait_for_retirement(retirement, wake_deadline()))
                .unwrap();
        });
        assert!(completion.try_recv().is_err());
        claim.finish(Ok(()));
        assert_eq!(
            completion.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(())
        );
    });
    assert!(tracker.lock().in_progress.is_empty());
}

#[test]
fn aborted_claim_releases_ownership_without_requesting_a_wake() {
    let tracker = new_restart_tracker();
    let claim = claim(&tracker, "tab").unwrap();
    assert!(!claim.wake_requested());
    claim.finish(Ok(()));
    assert!(request_wake(&tracker, "tab").is_none());
    assert!(super::claim(&tracker, "tab").is_ok());
}

#[test]
fn wake_deadline_is_failure_and_does_not_release_the_sleep_owner() {
    let tracker = new_restart_tracker();
    let claim = claim(&tracker, "tab").unwrap();
    let retirement = request_wake(&tracker, "tab");
    assert!(wait_for_retirement(retirement, Instant::now()).is_err());
    assert!(tracker.lock().in_progress.contains("tab"));
    claim.finish(Ok(()));
}

#[test]
fn interrupted_retirement_reports_failure_to_the_waiter() {
    let tracker = new_restart_tracker();
    let claim = claim(&tracker, "tab").unwrap();
    let retirement = request_wake(&tracker, "tab");
    drop(claim);
    assert!(wait_for_retirement(retirement, wake_deadline()).is_err());
    assert!(tracker.lock().in_progress.is_empty());
}
