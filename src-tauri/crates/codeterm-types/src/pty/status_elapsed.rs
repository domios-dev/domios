/// One current-versus-previous observation of a status widget's own elapsed
/// counter. `advancing` is the pair itself; `progressed` is sticky for the
/// lifetime of one widget, because a counter only ticks when the provider
/// repaints and a quiet gap between repaints must not read as a finished turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusElapsedProgress {
    pub previous_ms: Option<u64>,
    pub current_ms: Option<u64>,
    /// This widget has been seen advancing at some earlier observation. Cleared
    /// with the baseline, so a new or vanished widget inherits nothing.
    pub progressed: bool,
}

impl StatusElapsedProgress {
    /// No widget, no counter, no baseline — never progress evidence.
    pub const NONE: Self = Self {
        previous_ms: None,
        current_ms: None,
        progressed: false,
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
    progressed: bool,
}

impl StatusElapsedTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed this observation's elapsed value. A widget that is gone or carries no
    /// counter clears the baseline, and a counter that restarts lower is a new
    /// turn — both drop the previous turn's progress, so nothing can latch.
    pub fn observe(&mut self, current_ms: Option<u64>) -> StatusElapsedProgress {
        let Some(current_ms) = current_ms else {
            self.baseline_ms = None;
            self.progressed = false;
            return StatusElapsedProgress::NONE;
        };
        let previous_ms = self.baseline_ms.filter(|baseline| *baseline <= current_ms);
        if previous_ms.is_none() {
            self.progressed = false;
        }
        self.progressed |= previous_ms.is_some_and(|previous| current_ms > previous);
        self.baseline_ms = Some(current_ms);
        StatusElapsedProgress {
            previous_ms,
            current_ms: Some(current_ms),
            progressed: self.progressed,
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
    fn observed_progress_is_sticky_for_the_life_of_one_widget() {
        let mut tracker = StatusElapsedTracker::new();
        assert!(!tracker.observe(Some(14_000)).progressed);
        assert!(tracker.observe(Some(15_000)).progressed);

        let frozen = tracker.observe(Some(15_000));
        assert!(!frozen.advancing());
        assert!(
            frozen.progressed,
            "a frozen tick keeps the widget's progress"
        );
    }

    #[test]
    fn restart_and_disappearance_clear_the_baseline() {
        let mut tracker = StatusElapsedTracker::new();
        tracker.observe(Some(14_000));
        tracker.observe(Some(15_000));

        let restarted = tracker.observe(Some(2_000));
        assert_eq!(restarted.previous_ms, None);
        assert!(!restarted.advancing());
        assert!(!restarted.progressed, "a new turn inherits no progress");

        tracker.observe(Some(3_000));
        assert_eq!(tracker.observe(None), StatusElapsedProgress::NONE);
        let after_gap = tracker.observe(Some(9_000));
        assert!(!after_gap.advancing());
        assert!(!after_gap.progressed);
    }
}
