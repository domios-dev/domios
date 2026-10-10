use serde::Deserialize;
use std::collections::BTreeMap;

use super::{AUTOMATIC_ACTION_CONFIDENCE, HELDOUT_PRECISION_REQUIRED};

pub(crate) const AUTOMATIC_LABELS: [&str; 4] = [
    "provider_update_prompt",
    "crashed_to_shell",
    "still_starting",
    "error_loop",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Qualification {
    enabled: bool,
    live_oracle_verified: bool,
    adapter: String,
    model: Option<String>,
    criteria_hash: String,
    heldout_hash: String,
    threshold: f64,
    predicted: usize,
    correct: usize,
    human_labelled_support: BTreeMap<String, usize>,
}

impl Qualification {
    pub(crate) fn supports_label(&self, label: &str) -> bool {
        self.human_labelled_support.get(label).copied().unwrap_or(0) >= 4
    }

    pub(crate) fn authorizes(
        &self,
        adapter: &str,
        model: Option<&str>,
        criteria_hash: &str,
        heldout_hash: &str,
    ) -> bool {
        self.enabled
            && self.live_oracle_verified
            && self.adapter == adapter
            && self.model.as_deref() == model
            && model.is_some()
            && self.criteria_hash == criteria_hash
            && valid_hash(&self.heldout_hash)
            && self.heldout_hash == heldout_hash
            && self.threshold == AUTOMATIC_ACTION_CONFIDENCE
            && self.predicted >= 16
            && self.correct <= self.predicted
            && self.correct as f64 / self.predicted as f64 >= HELDOUT_PRECISION_REQUIRED
            && AUTOMATIC_LABELS
                .into_iter()
                .all(|label| self.human_labelled_support.get(label).copied().unwrap_or(0) >= 4)
            && ["ready_idle", "working_normally", "other"]
                .into_iter()
                .all(|label| self.human_labelled_support.get(label).copied().unwrap_or(0) > 0)
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn recorded_qualification() -> Option<Qualification> {
    serde_json::from_str(include_str!(
        "../../../../../evals/tab-state-jev/qualification.json"
    ))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ga_heldout_requires_precision_coverage_identity_and_live_oracle() {
        let mut q = Qualification {
            enabled: true,
            live_oracle_verified: true,
            adapter: "fixture".into(),
            model: Some("fixture-model".into()),
            criteria_hash: "criteria".into(),
            heldout_hash: "a".repeat(64),
            threshold: 0.90,
            predicted: 20,
            correct: 19,
            human_labelled_support: AUTOMATIC_LABELS
                .into_iter()
                .map(|label| (label.into(), 4))
                .chain(
                    ["ready_idle", "working_normally", "other"]
                        .into_iter()
                        .map(|label| (label.into(), 1)),
                )
                .collect(),
        };
        assert!(q.authorizes(
            "fixture",
            Some("fixture-model"),
            "criteria",
            &"a".repeat(64)
        ));
        assert!(!q.authorizes(
            "fixture",
            Some("fixture-model"),
            "criteria",
            &"b".repeat(64)
        ));
        q.correct = 18;
        assert!(!q.authorizes(
            "fixture",
            Some("fixture-model"),
            "criteria",
            &"a".repeat(64)
        ));
        q.correct = 19;
        assert!(!q.authorizes("fixture", Some("other-model"), "criteria", &"a".repeat(64)));
        assert!(!q.authorizes("fixture", None, "criteria", &"a".repeat(64)));
        q.live_oracle_verified = false;
        assert!(!q.authorizes(
            "fixture",
            Some("fixture-model"),
            "criteria",
            &"a".repeat(64)
        ));
        q.live_oracle_verified = true;
        q.human_labelled_support.insert("error_loop".into(), 3);
        assert!(!q.authorizes(
            "fixture",
            Some("fixture-model"),
            "criteria",
            &"a".repeat(64)
        ));
    }

    #[test]
    fn ga_shipped_qualification_stays_disabled_without_human_evidence() {
        let q = recorded_qualification().expect("qualification receipt parses");
        assert!(!q.authorizes(
            "fixture",
            Some("fixture-model"),
            "criteria",
            &"a".repeat(64)
        ));
    }
}
