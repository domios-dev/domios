use super::*;

#[test]
fn replacement_crash_during_restart_stops_instead_of_being_ignored() {
    let mut recovery = Recovery::default();
    let now = Instant::now();
    assert_eq!(recovery.claim("first", now), Decision::Restart);
    assert_eq!(recovery.claim("replacement", now), Decision::Incident);
    assert_eq!(recovery.claim("replacement", now), Decision::Ignore);
}

#[test]
fn missing_turn_start_notifies_once_without_claiming_a_dead_channel_or_restarting() {
    let mut recovery = Recovery::default();
    assert!(recovery.observe_stall("own"));
    assert!(!recovery.observe_stall("own"));
    assert!(!recovery.in_flight);
    assert!(recovery.last_attempt.is_none());
    assert_eq!(recovery.claim("own", Instant::now()), Decision::Restart);
}

#[test]
fn dead_epoch_gets_one_restart_and_new_epoch_respects_cooldown() {
    let mut recovery = Recovery::default();
    let now = Instant::now();
    assert_eq!(recovery.claim("nonce:1", now), Decision::Restart);
    assert_eq!(recovery.claim("nonce:1", now), Decision::Ignore);
    recovery.in_flight = false;
    assert_eq!(recovery.claim("nonce:1", now + COOLDOWN), Decision::Ignore);
    assert_eq!(recovery.claim("nonce:2", now), Decision::Incident);
    assert_eq!(recovery.claim("nonce:2", now + COOLDOWN), Decision::Ignore);
    assert_eq!(recovery.claim("nonce:3", now + COOLDOWN), Decision::Restart);
}

#[test]
fn failed_restart_cannot_schedule_another_attempt_for_the_epoch() {
    let mut recovery = Recovery::default();
    let now = Instant::now();
    assert_eq!(recovery.claim("failed", now), Decision::Restart);
    recovery.incident_epoch = Some("failed-replacement".into());
    recovery.in_flight = false;
    for elapsed in [Duration::ZERO, COOLDOWN, COOLDOWN * 2] {
        assert_eq!(recovery.claim("failed", now + elapsed), Decision::Ignore);
    }
    assert_eq!(
        recovery.claim("failed-replacement", now + COOLDOWN),
        Decision::Ignore
    );
}
