use super::{
    machine::{Machine, Outcome},
    probe::Output,
    provenance::{Evidence, Scope},
};
use serde::{Deserialize, Serialize};

pub fn evaluate_probe(
    machine: &mut Machine,
    output: &Output,
    scope: Scope,
    evaluate: impl FnOnce(&mut Machine, Scope) -> Outcome,
) -> Outcome {
    if output.success {
        return evaluate(machine, scope);
    }
    machine.last_hash = None;
    Outcome {
        evidence: Evidence::new(scope, None),
        reason: "probe_command_failed",
        verdict: None,
        route: None,
        handlers: Vec::new(),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub runs_since_transition: u64,
    pub last_probe_at: Option<u64>,
    pub probe_state: Option<String>,
}

impl Progress {
    pub fn record(&mut self, now: u64, state: Option<&str>, transition: bool) {
        let changed = state.is_some() && self.probe_state.as_deref() != state;
        self.runs_since_transition = if transition || changed {
            0
        } else {
            self.runs_since_transition.saturating_add(1)
        };
        self.last_probe_at = Some(now);
        if let Some(state) = state {
            self.probe_state = Some(state.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_probe_bypasses_classifier_and_clears_unchanged_hash() {
        let mut machine = Machine {
            last_hash: Some("old".into()),
            ..Machine::default()
        };
        let scope = Scope {
            sentinel_id: "watch".into(),
            creator_tab_id: "creator".into(),
            team_id: None,
            workspace_id: "ws".into(),
        };
        for code in [Some(1), Some(127), None] {
            let output = Output {
                success: false,
                exit_code: code,
                text: "waiting".into(),
                ..Output::default()
            };
            let outcome = evaluate_probe(&mut machine, &output, scope.clone(), |_, _| {
                panic!("failed probes must not classify")
            });
            assert_eq!(outcome.reason, "probe_command_failed");
            assert!(machine.last_hash.is_none());
            assert!(outcome.handlers.is_empty());
        }
    }

    #[test]
    fn probe_progress_counts_repeats_and_resets_every_transition() {
        let mut progress = Progress::default();
        progress.record(1000, Some("waiting"), true);
        assert_eq!(progress.runs_since_transition, 0);
        progress.record(61000, None, false);
        progress.record(121000, Some("waiting"), false);
        assert_eq!(progress.runs_since_transition, 2);
        assert_eq!(progress.last_probe_at, Some(121000));
        progress.record(181000, Some("waiting"), true);
        assert_eq!(progress.runs_since_transition, 0);
        progress.record(241000, Some("done"), true);
        assert_eq!(progress.probe_state.as_deref(), Some("done"));
        assert_eq!(progress.runs_since_transition, 0);
    }

    #[test]
    fn probe_progress_errors_and_unmatched_results_are_visible() {
        let mut progress = Progress::default();
        for state in ["waiting", "error", "other", "waiting"] {
            progress.record(1000, Some(state), false);
            assert_eq!(progress.probe_state.as_deref(), Some(state));
            assert_eq!(progress.runs_since_transition, 0);
            progress.record(2000, None, false);
            assert_eq!(progress.runs_since_transition, 1);
        }
    }
}
