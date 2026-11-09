use super::*;

const MINUTE_MS: u64 = 60_000;
const START_MS: u64 = 1_800_000_000_000;

#[test]
fn the_throttle_holds_across_restarts() {
    let root = tempfile::tempdir().unwrap();
    assert!(CheckThrottle::in_root(root.path()).admit(CheckTrigger::Automatic, START_MS));
    for later in [1_u64, 10, 28] {
        // A new value over the same directory is what a restarted process sees.
        let restarted = CheckThrottle::in_root(root.path());
        assert!(!restarted.due(START_MS + later * MINUTE_MS), "{later}");
        assert!(
            !restarted.admit(CheckTrigger::Automatic, START_MS + later * MINUTE_MS),
            "{later}"
        );
    }
    assert!(CheckThrottle::in_root(root.path())
        .admit(CheckTrigger::Automatic, START_MS + 30 * MINUTE_MS));
}

#[test]
fn automatic_checks_run_at_most_about_twice_an_hour_however_often_they_are_asked() {
    let root = tempfile::tempdir().unwrap();
    let throttle = CheckThrottle::in_root(root.path());
    let admitted = (0..120_u64)
        .filter(|minute| throttle.admit(CheckTrigger::Automatic, START_MS + minute * MINUTE_MS))
        .count();
    // 29-minute spacing over two hours: minutes 0, 29, 58, 87 and 116.
    assert_eq!(admitted, 5);
}

#[test]
fn a_thirty_minute_timer_tick_is_not_skipped_by_its_own_previous_attempt() {
    let root = tempfile::tempdir().unwrap();
    let throttle = CheckThrottle::in_root(root.path());
    assert!(throttle.admit(CheckTrigger::Automatic, START_MS + 500));
    assert!(throttle.admit(CheckTrigger::Automatic, START_MS + 30 * MINUTE_MS));
}

#[test]
fn an_explicit_check_bypasses_the_throttle_and_restarts_its_interval() {
    let root = tempfile::tempdir().unwrap();
    let throttle = CheckThrottle::in_root(root.path());
    assert!(throttle.admit(CheckTrigger::Automatic, START_MS));
    assert!(throttle.admit(CheckTrigger::Explicit, START_MS + MINUTE_MS));
    assert!(!throttle.admit(CheckTrigger::Automatic, START_MS + 29 * MINUTE_MS));
    assert!(throttle.admit(CheckTrigger::Automatic, START_MS + 30 * MINUTE_MS));
}

#[test]
fn a_clock_moved_backward_cannot_silence_checks() {
    let root = tempfile::tempdir().unwrap();
    let throttle = CheckThrottle::in_root(root.path());
    assert!(throttle.admit(CheckTrigger::Automatic, START_MS));
    assert!(throttle.admit(CheckTrigger::Automatic, START_MS - 60 * MINUTE_MS));
}

#[test]
fn an_unreadable_state_file_never_blocks_a_check() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("check-throttle.json"), b"not json").unwrap();
    assert!(CheckThrottle::in_root(root.path()).admit(CheckTrigger::Automatic, START_MS));
}

#[test]
fn force_maps_to_the_explicit_trigger() {
    assert_eq!(CheckTrigger::from_force(true), CheckTrigger::Explicit);
    assert_eq!(CheckTrigger::from_force(false), CheckTrigger::Automatic);
}
