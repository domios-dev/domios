use serde::{Deserialize, Serialize};

pub const MIN_INTERVAL_MS: u64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    pub every_ms: u64,
    pub until_ms: Option<u64>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextOccurrence {
    pub scheduled_at_ms: Option<u64>,
    pub tick_index: u64,
    pub skipped_ticks: u64,
}

pub fn should_skip_tick(
    previous_tick_unconfirmed: bool,
    pending_delivery: bool,
    unresolved_send: bool,
    tab_busy: bool,
) -> bool {
    previous_tick_unconfirmed || pending_delivery || unresolved_send || tab_busy
}

pub fn validate(rule: &RecurrenceRule, start_at_ms: u64) -> Result<(), String> {
    if rule.every_ms < MIN_INTERVAL_MS {
        return Err("--every must be at least 1m".into());
    }
    match (rule.until_ms, rule.count) {
        (Some(_), Some(_)) => Err("use only one of --until or --count".into()),
        (Some(until), None) if until < start_at_ms => {
            Err("--until must be at or after the first scheduled fire".into())
        }
        (None, Some(0)) => Err("--count must be greater than zero".into()),
        _ => Ok(()),
    }
}

pub fn scheduled_at(start_at_ms: u64, every_ms: u64, tick_index: u64) -> Option<u64> {
    every_ms
        .checked_mul(tick_index)
        .and_then(|offset| start_at_ms.checked_add(offset))
}

pub fn after_tick(
    start_at_ms: u64,
    rule: &RecurrenceRule,
    tick_index: u64,
    now_ms: u64,
    current_tick_skipped: bool,
) -> NextOccurrence {
    if rule.every_ms < MIN_INTERVAL_MS {
        return NextOccurrence {
            scheduled_at_ms: None,
            tick_index,
            skipped_ticks: 0,
        };
    }
    let first_unprocessed = tick_index.saturating_add(1);
    let elapsed = now_ms.saturating_sub(start_at_ms);
    let first_not_before_now = elapsed
        .checked_div(rule.every_ms)
        .unwrap_or_default()
        .saturating_add(if elapsed % rule.every_ms != 0 { 1 } else { 0 });
    let skip_due_slot = current_tick_skipped
        && elapsed % rule.every_ms == 0
        && elapsed / rule.every_ms >= first_unprocessed;
    let next_index = first_unprocessed.max(
        first_not_before_now.saturating_add(if skip_due_slot { 1 } else { 0 }),
    );
    let missed_before_next = next_index.saturating_sub(first_unprocessed);
    let skipped_ticks =
        missed_before_next.saturating_add(if current_tick_skipped { 1 } else { 0 });
    let next_at = scheduled_at(start_at_ms, rule.every_ms, next_index).filter(|scheduled| {
        rule.until_ms.is_none_or(|until| *scheduled <= until)
            && rule.count.is_none_or(|count| next_index < u64::from(count))
    });
    NextOccurrence {
        scheduled_at_ms: next_at,
        tick_index: next_index,
        skipped_ticks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(every_ms: u64, until_ms: Option<u64>, count: Option<u32>) -> RecurrenceRule {
        RecurrenceRule {
            every_ms,
            until_ms,
            count,
        }
    }

    #[test]
    fn validates_interval_and_exclusive_stop_conditions() {
        assert!(validate(&rule(59_999, None, Some(2)), 100_000).is_err());
        assert!(validate(&rule(60_000, Some(200_000), Some(2)), 100_000).is_err());
        assert!(validate(&rule(60_000, None, None), 100_000).is_ok());
        assert!(validate(&rule(60_000, None, Some(0)), 100_000).is_err());
        assert!(validate(&rule(60_000, Some(200_000), None), 100_000).is_ok());
    }

    #[test]
    fn recurrence_next_fire_is_anchored_to_series_start() {
        let schedule = after_tick(10_000, &rule(60_000, None, Some(8)), 0, 151_000, false);
        assert_eq!(schedule.scheduled_at_ms, Some(190_000));
        assert_eq!(schedule.tick_index, 3);
        assert_eq!(schedule.skipped_ticks, 2);
    }

    #[test]
    fn occurrence_due_at_delivery_completion_is_not_drifted() {
        let schedule = after_tick(10_000, &rule(60_000, None, Some(4)), 0, 70_000, false);
        assert_eq!(schedule.scheduled_at_ms, Some(70_000));
        assert_eq!(schedule.tick_index, 1);
        assert_eq!(schedule.skipped_ticks, 0);
    }

    #[test]
    fn recurring_tick_is_skipped_while_previous_is_pending() {
        assert!(should_skip_tick(false, false, true, false));
        let schedule = after_tick(10_000, &rule(60_000, None, Some(4)), 0, 70_000, true);
        assert_eq!(schedule.scheduled_at_ms, Some(130_000));
        assert_eq!(schedule.tick_index, 2);
        assert_eq!(schedule.skipped_ticks, 2);
    }

    #[test]
    fn a_busy_tick_is_counted_and_the_next_slot_is_anchored() {
        let schedule = after_tick(10_000, &rule(60_000, None, Some(4)), 0, 130_000, true);
        assert_eq!(schedule.scheduled_at_ms, Some(190_000));
        assert_eq!(schedule.tick_index, 3);
        assert_eq!(schedule.skipped_ticks, 3);
    }

    #[test]
    fn until_and_count_remove_the_series_after_the_last_slot() {
        assert_eq!(
            after_tick(10_000, &rule(60_000, None, Some(1)), 0, 10_000, false)
                .scheduled_at_ms,
            None
        );
        assert_eq!(
            after_tick(10_000, &rule(60_000, Some(69_999), None), 0, 10_000, false)
                .scheduled_at_ms,
            None
        );
    }

    #[test]
    fn pending_or_unprocessed_delivery_skips_the_next_tick() {
        assert!(should_skip_tick(false, true, false, false));
        assert!(should_skip_tick(false, false, true, false));
        assert!(should_skip_tick(false, false, false, true));
        assert!(should_skip_tick(true, false, false, false));
        assert!(!should_skip_tick(false, false, false, false));
    }
}
