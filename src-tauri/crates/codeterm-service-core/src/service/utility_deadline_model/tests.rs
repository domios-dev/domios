use super::*;

#[test]
fn each_command_start_resets_the_deadline() {
    let mut model = DeadlineModel::default();
    model.set_timeout(Some(1_800_000)).unwrap();
    model.command_started(1_000).unwrap();
    assert!(!model.expired(1_800_999));
    assert!(model.expired(1_801_000));
    model.command_finished();
    model.command_started(2_000_000).unwrap();
    assert_eq!(model.deadline_ms, Some(3_800_000));
    assert!(!model.expired(3_799_999));
    assert!(model.expired(3_800_000));
    assert_eq!(elapsed_seconds(2_000_000, 3_800_000), 1800.0);
}

#[test]
fn idle_has_no_deadline_even_after_previous_commands() {
    let mut model = DeadlineModel::default();
    model.set_timeout(Some(30)).unwrap();
    assert!(!model.expired(u64::MAX));
    model.command_started(100).unwrap();
    model.command_finished();
    assert_eq!(model.started_ms, None);
    assert_eq!(model.deadline_ms, None);
    assert!(!model.expired(u64::MAX));
}

#[test]
fn timeout_changes_apply_to_current_start_and_future_commands() {
    let mut model = DeadlineModel::default();
    model.set_timeout(Some(30)).unwrap();
    model.command_started(100).unwrap();
    model.set_timeout(Some(60)).unwrap();
    assert!(!model.expired(130));
    assert!(model.expired(160));
    model.set_timeout(Some(20)).unwrap();
    assert!(model.expired(130));
    model.command_finished();
    model.command_started(1_000).unwrap();
    assert_eq!(model.deadline_ms, Some(1_020));
}

#[test]
fn none_disables_current_and_future_deadlines() {
    let mut model = DeadlineModel::default();
    model.set_timeout(Some(30)).unwrap();
    model.command_started(100).unwrap();
    model.set_timeout(None).unwrap();
    assert!(!model.expired(u64::MAX));
    model.command_finished();
    model.command_started(1_000).unwrap();
    assert!(!model.expired(u64::MAX));
    model.set_timeout(Some(50)).unwrap();
    assert_eq!(model.deadline_ms, Some(1_050));
}

#[test]
fn timeout_duration_is_bounded_and_explicit() {
    assert_eq!(parse_timeout("30s"), Ok(Some(30_000)));
    assert_eq!(parse_timeout("60m"), Ok(Some(3_600_000)));
    assert_eq!(parse_timeout("1h"), Ok(Some(3_600_000)));
    assert_eq!(parse_timeout("none"), Ok(None));
    for invalid in ["0s", "30", "-1s", "18446744073709551615h", "366d"] {
        assert!(parse_timeout(invalid).is_err());
    }
    assert!(deadline_from_start(u64::MAX, 1).is_err());
}
