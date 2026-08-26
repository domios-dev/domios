//! Which interval a readiness report attributes its changed files to.
//!
//! Three candidate bases reach a report and they are not equal in authority. An
//! explicitly typed `--base` is a caller naming an interval on purpose. The live
//! attempt base is what the plan record says this tab is working from right now.
//! The dispatch-time `CODETERM_LANE_BASE` is a snapshot of the attempt that
//! launched the shell — it goes stale the instant the tab is re-dispatched on a
//! new base, because the exported variable does not move with the attempt.

/// A `--base` value paired with the environment default clap may have filled it
/// from, so the two can be told apart at the point precedence is decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportBase {
    explicit: Option<String>,
    environment: Option<String>,
}

fn nonempty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl ReportBase {
    /// `--base` carries `env = "CODETERM_LANE_BASE"`, so an absent flag arrives as
    /// the variable's value and is indistinguishable from it. A supplied value
    /// equal to the variable is therefore classified as environment-supplied; a
    /// caller who types the dispatch base verbatim gets the dispatch base's
    /// precedence, which is the only reading the flag's own value supports.
    pub fn classify(supplied: Option<String>, environment: Option<String>) -> Self {
        let supplied = nonempty(supplied);
        let environment = nonempty(environment);
        if supplied.is_some() && supplied == environment {
            return Self {
                explicit: None,
                environment,
            };
        }
        Self {
            explicit: supplied,
            environment,
        }
    }

    #[cfg(test)]
    pub fn typed(base: &str) -> Self {
        Self::classify(Some(base.to_string()), None)
    }

    /// Explicit intent, then the live attempt, then the dispatch snapshot. The
    /// environment stays as a last resort so a report with no resolvable plan
    /// task still attributes over the interval its launch knew about.
    pub fn resolve(&self, live_attempt_base: Option<String>) -> Option<String> {
        self.explicit
            .clone()
            .or_else(|| nonempty(live_attempt_base))
            .or_else(|| self.environment.clone())
    }
}

#[cfg(test)]
mod tests;
