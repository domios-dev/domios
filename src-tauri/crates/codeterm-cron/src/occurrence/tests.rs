use super::*;
use crate::clock::{Clock, MockClock};

#[test]
fn jitter_and_lateness_are_measured_separately() {
    let occurrence = Occurrence {
        scheduled_at: 60_000,
        due_at: 72_000,
    };
    let clock = MockClock::starting_at(occurrence.scheduled_at);
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, false),
        FireDecision::WaitUntil(72_000)
    );
    clock.set_ms(occurrence.due_at + LATE_FIRE_MS);
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, false),
        FireDecision::Fire { late_by_ms: None }
    );
    clock.advance_ms(1);
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, false),
        FireDecision::Fire {
            late_by_ms: Some(LATE_FIRE_MS + 1)
        }
    );
}

#[test]
fn overlapping_run_only_blocks_its_own_job() {
    let occurrence = Occurrence {
        scheduled_at: 60_000,
        due_at: 60_000,
    };
    let clock = MockClock::starting_at(occurrence.due_at);
    let decisions =
        [true, false, false].map(|active| occurrence.decision(clock.now_ms(), false, active));
    assert_eq!(
        decisions,
        [
            FireDecision::WaitUntil(90_000),
            FireDecision::Fire { late_by_ms: None },
            FireDecision::Fire { late_by_ms: None },
        ]
    );
    clock.advance_ms(8_000);
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, false),
        FireDecision::Fire {
            late_by_ms: Some(8_000)
        }
    );
}

#[test]
fn recurring_occurrence_expires_even_with_a_run_in_progress() {
    let occurrence = Occurrence {
        scheduled_at: 60_000,
        due_at: 60_000,
    };
    let clock = MockClock::starting_at(occurrence.due_at + STALE_FIRE_MS);
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, true),
        FireDecision::WaitUntil(occurrence.due_at + STALE_FIRE_MS + 1)
    );
    clock.advance_ms(1);
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, true),
        FireDecision::Missed
    );
    assert_eq!(
        occurrence.decision(clock.now_ms(), false, false),
        FireDecision::Missed
    );
    assert_eq!(
        occurrence.decision(clock.now_ms(), true, false),
        FireDecision::Fire {
            late_by_ms: Some(STALE_FIRE_MS + 1)
        }
    );
}
