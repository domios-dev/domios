use super::*;

const CAP: usize = 16;

fn chunks(prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|i| format!("{prefix}-{i}")).collect()
}

fn drain(schedule: &mut EmbedSchedule) -> Vec<String> {
    let mut texts = Vec::new();
    while let Some(unit) = schedule.next_unit() {
        schedule.finish(&unit);
        texts.push(unit.text);
    }
    texts
}

#[test]
fn new_item_is_served_before_a_large_backlog_continues() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("big", chunks("big", 3000), CAP);
    for expected in 0..3 {
        let unit = schedule.next_unit().unwrap();
        assert_eq!(unit.text, format!("big-{expected}"));
        schedule.finish(&unit);
    }

    schedule.submit("small", chunks("small", 1), CAP);
    let unit = schedule.next_unit().unwrap();
    assert_eq!(unit.id, "small");
    assert!(unit.is_last);
    schedule.finish(&unit);

    assert_eq!(schedule.remaining_chunks("big"), 2997);
    assert_eq!(schedule.next_unit().unwrap().text, "big-3");
}

#[test]
fn backlogs_are_served_round_robin() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("a", chunks("a", 3), CAP);
    schedule.submit("b", chunks("b", 2), CAP);
    assert_eq!(drain(&mut schedule), ["b-0", "a-0", "b-1", "a-1", "a-2"]);
    assert_eq!(schedule.tracked_items(), 0);
}

#[test]
fn cancel_drops_pending_chunks_and_stales_the_unit_in_flight() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("doomed", chunks("doomed", 5), CAP);
    schedule.submit("kept", chunks("kept", 1), CAP);
    let kept = schedule.next_unit().unwrap();
    let in_flight = schedule.next_unit().unwrap();
    assert_eq!(in_flight.id, "doomed");

    assert_eq!(schedule.cancel("doomed"), 4);
    assert!(!schedule.is_current(&in_flight));
    assert!(schedule.is_current(&kept));
    schedule.finish(&in_flight);
    schedule.finish(&kept);
    assert!(schedule.next_unit().is_none());
    assert_eq!(schedule.tracked_items(), 0);
}

#[test]
fn cancelling_the_last_unit_in_flight_stales_it() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("one", chunks("one", 1), CAP);
    let unit = schedule.next_unit().unwrap();
    assert!(schedule.is_current(&unit));
    schedule.cancel("one");
    assert!(!schedule.is_current(&unit));
}

#[test]
fn resubmit_supersedes_the_older_generation() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("item", chunks("old", 4), CAP);
    let old = schedule.next_unit().unwrap();

    let outcome = schedule.submit("item", chunks("new", 2), CAP);
    assert!(matches!(
        outcome,
        SubmitOutcome::Queued {
            superseded: true,
            ..
        }
    ));
    assert!(!schedule.is_current(&old));
    assert_eq!(schedule.abandon(&old), 0);
    schedule.finish(&old);

    let fresh = schedule.next_unit().unwrap();
    assert_eq!((fresh.text.as_str(), fresh.chunk_index), ("new-0", 0));
    assert!(schedule.is_current(&fresh));
    schedule.finish(&fresh);
    assert_eq!(drain(&mut schedule), ["new-1"]);
}

#[test]
fn abandon_drops_the_rest_of_the_current_generation() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("gone", chunks("gone", 4), CAP);
    let unit = schedule.next_unit().unwrap();
    assert_eq!(schedule.abandon(&unit), 3);
    schedule.finish(&unit);
    assert!(schedule.next_unit().is_none());
}

#[test]
fn capacity_bounds_distinct_items_but_not_resubmits() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("a", chunks("a", 1), 1);
    assert_eq!(schedule.submit("b", chunks("b", 1), 1), SubmitOutcome::Full);
    assert!(matches!(
        schedule.submit("a", chunks("a2", 1), 1),
        SubmitOutcome::Queued {
            superseded: true,
            ..
        }
    ));
}

#[test]
fn empty_submit_cancels_pending_chunks() {
    let mut schedule = EmbedSchedule::default();
    schedule.submit("item", chunks("item", 3), CAP);
    schedule.submit("item", Vec::new(), CAP);
    assert!(schedule.is_idle());
    assert_eq!(schedule.tracked_items(), 0);
}
