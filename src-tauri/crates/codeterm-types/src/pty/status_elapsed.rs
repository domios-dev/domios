/// One current-versus-previous observation of a status widget's own elapsed
/// counter. `advancing` is structural progress evidence: the turn moved between
/// two observations, which a quiescence clock measured from *now* cannot see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusElapsedProgress {
    pub previous_ms: Option<u64>,
    pub current_ms: Option<u64>,
}

impl StatusElapsedProgress {
    /// No widget, no counter, no baseline — never progress evidence.
    pub const NONE: Self = Self {
        previous_ms: None,
        current_ms: None,
    };

    pub fn advancing(self) -> bool {
        match (self.previous_ms, self.current_ms) {
            (Some(previous), Some(current)) => current > previous,
            _ => false,
        }
    }
}

/// The one elapsed baseline both adapters keep, so a live tab and a replayed
/// cast derive identical progress from an identical observation sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusElapsedTracker {
    baseline_ms: Option<u64>,
}

impl StatusElapsedTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed this observation's elapsed value. A widget that is gone or carries no
    /// counter clears the baseline, and a counter that restarts lower is a new
    /// turn — neither may report progress, so a stale value can never latch.
    pub fn observe(&mut self, current_ms: Option<u64>) -> StatusElapsedProgress {
        let Some(current_ms) = current_ms else {
            self.baseline_ms = None;
            return StatusElapsedProgress::NONE;
        };
        let previous_ms = self.baseline_ms.filter(|baseline| *baseline <= current_ms);
        self.baseline_ms = Some(current_ms);
        StatusElapsedProgress {
            previous_ms,
            current_ms: Some(current_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StatusElapsedProgress, StatusElapsedTracker};

    #[test]
    fn advance_reports_progress_and_freeze_does_not() {
        let mut tracker = StatusElapsedTracker::new();
        assert!(!tracker.observe(Some(14_000)).advancing());
        assert!(tracker.observe(Some(15_000)).advancing());
        assert!(!tracker.observe(Some(15_000)).advancing());
    }

    #[test]
    fn restart_and_disappearance_clear_the_baseline() {
        let mut tracker = StatusElapsedTracker::new();
        tracker.observe(Some(15_000));
        let restarted = tracker.observe(Some(2_000));
        assert_eq!(restarted.previous_ms, None);
        assert!(!restarted.advancing());

        tracker.observe(Some(3_000));
        assert_eq!(tracker.observe(None), StatusElapsedProgress::NONE);
        assert!(!tracker.observe(Some(9_000)).advancing());
    }
}
