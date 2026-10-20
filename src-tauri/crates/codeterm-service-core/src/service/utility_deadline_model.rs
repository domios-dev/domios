use crate::multiplexer::types::ShellActivity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeadlineAction {
    QuietClose,
    TerminateAndNotice,
}

#[derive(Default)]
pub(crate) struct DeadlineModel {
    deadline_ms: Option<u64>,
}

impl DeadlineModel {
    pub(crate) fn reset(&mut self, deadline_ms: Option<u64>) {
        self.deadline_ms = deadline_ms;
    }

    pub(crate) fn expire(
        &mut self,
        now_ms: u64,
        activity: ShellActivity,
    ) -> Option<DeadlineAction> {
        if self.deadline_ms.is_none_or(|deadline| now_ms < deadline) {
            return None;
        }
        self.deadline_ms = None;
        Some(if activity == ShellActivity::Idle {
            DeadlineAction::QuietClose
        } else {
            DeadlineAction::TerminateAndNotice
        })
    }
}

pub fn parse_timeout(value: &str) -> Result<Option<u64>, String> {
    if value == "none" {
        return Ok(None);
    }
    let (number, multiplier) = match value.as_bytes().last() {
        Some(b's') => (&value[..value.len() - 1], 1_000),
        Some(b'm') => (&value[..value.len() - 1], 60_000),
        Some(b'h') => (&value[..value.len() - 1], 3_600_000),
        _ => return Err("timeout requires a positive duration with s, m or h, or none".into()),
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(multiplier))
        .filter(|ms| *ms > 0 && *ms <= 365 * 24 * 3_600_000)
        .map(Some)
        .ok_or_else(|| "timeout must be positive and at most 365 days".into())
}

pub(crate) fn deadline_from_start(started_ms: u64, duration_ms: u64) -> Result<u64, String> {
    started_ms
        .checked_add(duration_ms)
        .ok_or_else(|| "deadline overflow".into())
}

pub(crate) fn elapsed_seconds(started_ms: u64, now_ms: u64) -> f64 {
    now_ms.saturating_sub(started_ms) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_closes_quietly_and_running_or_unknown_notices_once() {
        for activity in [
            ShellActivity::Idle,
            ShellActivity::Running,
            ShellActivity::Unknown,
        ] {
            let mut model = DeadlineModel::default();
            model.reset(Some(30));
            assert_eq!(model.expire(29, activity), None);
            assert_eq!(
                model.expire(30, activity),
                Some(if activity == ShellActivity::Idle {
                    DeadlineAction::QuietClose
                } else {
                    DeadlineAction::TerminateAndNotice
                })
            );
            assert_eq!(model.expire(31, activity), None);
        }
    }

    #[test]
    fn resetting_extends_or_shortens_and_none_disables() {
        let mut model = DeadlineModel::default();
        assert_eq!(model.expire(u64::MAX, ShellActivity::Running), None);
        model.reset(Some(30));
        model.reset(Some(60));
        assert_eq!(model.expire(30, ShellActivity::Running), None);
        model.reset(Some(40));
        assert_eq!(
            model.expire(40, ShellActivity::Idle),
            Some(DeadlineAction::QuietClose)
        );
        model.reset(Some(90));
        model.reset(None);
        assert_eq!(model.expire(u64::MAX, ShellActivity::Unknown), None);
    }

    #[test]
    fn timeout_duration_is_bounded_and_explicit() {
        assert_eq!(parse_timeout("30s"), Ok(Some(30_000)));
        assert_eq!(parse_timeout("2m"), Ok(Some(120_000)));
        assert_eq!(parse_timeout("1h"), Ok(Some(3_600_000)));
        assert_eq!(parse_timeout("none"), Ok(None));
        for invalid in ["0s", "30", "-1s", "18446744073709551615h", "366d"] {
            assert!(parse_timeout(invalid).is_err());
        }
    }

    #[test]
    fn requested_lifetime_and_command_elapsed_use_their_own_starts() {
        let started = 1_792_013_689_000;
        let duration = parse_timeout("30m").unwrap().unwrap();
        let deadline = deadline_from_start(started, duration).unwrap();
        assert_eq!(deadline, started + 1_800_000);
        assert_eq!(elapsed_seconds(started, deadline), 1800.0);
        assert_eq!(elapsed_seconds(deadline - 197_000, deadline), 197.0);
        let mut model = DeadlineModel::default();
        model.reset(Some(deadline));
        assert_eq!(model.expire(deadline - 1, ShellActivity::Running), None);
        assert_eq!(
            model.expire(deadline, ShellActivity::Running),
            Some(DeadlineAction::TerminateAndNotice)
        );
        assert!(deadline_from_start(u64::MAX, duration).is_err());
    }
}
