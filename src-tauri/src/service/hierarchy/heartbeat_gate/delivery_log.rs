use serde::Serialize;

const MAX_TICKS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TickOutcome {
    Sent,
    Skipped,
    Suppressed,
}

impl TickOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Skipped => "skipped",
            Self::Suppressed => "suppressed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpinalVerdict {
    Processed,
    Skipped,
    Suppressed,
}

impl SpinalVerdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Processed => "processed",
            Self::Skipped => "skipped",
            Self::Suppressed => "suppressed",
        }
    }
}

/// What the existing reminder pass already decided. Classification never
/// schedules another tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TickFact {
    NotDue,
    UnchangedContent,
    Unavailable,
    Accepted { decided_by: String },
    Pending,
    Failed,
    Suppressed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeliveryTick {
    pub at_ms: u64,
    pub outcome: TickOutcome,
    pub spinal_verdict: SpinalVerdict,
    pub spinal_reason: String,
    pub tab_id: String,
    pub plan_id: String,
}

pub(crate) fn classify_tick(fact: &TickFact) -> (TickOutcome, SpinalVerdict, String) {
    match fact {
        TickFact::NotDue => (
            TickOutcome::Skipped,
            SpinalVerdict::Skipped,
            "not_due".into(),
        ),
        TickFact::UnchangedContent => (
            TickOutcome::Skipped,
            SpinalVerdict::Skipped,
            "unchanged_content".into(),
        ),
        TickFact::Unavailable => (
            TickOutcome::Suppressed,
            SpinalVerdict::Skipped,
            "agent_unavailable".into(),
        ),
        TickFact::Accepted { decided_by } if decided_by == "cadence" => (
            TickOutcome::Sent,
            SpinalVerdict::Skipped,
            "actionable_cadence".into(),
        ),
        TickFact::Accepted { .. } => (
            TickOutcome::Sent,
            SpinalVerdict::Processed,
            "delivered".into(),
        ),
        TickFact::Pending => (
            TickOutcome::Skipped,
            SpinalVerdict::Processed,
            "pending".into(),
        ),
        TickFact::Failed => (
            TickOutcome::Suppressed,
            SpinalVerdict::Processed,
            "delivery_failed".into(),
        ),
        TickFact::Suppressed { reason } => {
            let spinal = if spinal_not_asked(reason) {
                SpinalVerdict::Skipped
            } else {
                SpinalVerdict::Suppressed
            };
            let outcome = if matches!(reason.as_str(), "in_flight" | "scope_capacity") {
                TickOutcome::Skipped
            } else {
                TickOutcome::Suppressed
            };
            (outcome, spinal, reason.clone())
        }
    }
}

fn spinal_not_asked(reason: &str) -> bool {
    matches!(
        reason,
        "no_adapter"
            | "in_flight"
            | "scope_capacity"
            | "recipient_unavailable"
            | "plan_missing"
            | "ineligible"
            | "binding_missing"
            | "binding_ambiguous"
            | "gate_unavailable"
    )
}

pub(crate) fn delivery_tick(tab: &str, plan: &str, now_secs: u64, fact: TickFact) -> DeliveryTick {
    let (outcome, spinal_verdict, spinal_reason) = classify_tick(&fact);
    DeliveryTick {
        at_ms: now_secs.saturating_mul(1000),
        outcome,
        spinal_verdict,
        spinal_reason,
        tab_id: tab.to_string(),
        plan_id: plan.to_string(),
    }
}

pub(crate) fn push_tick(ticks: &mut std::collections::VecDeque<DeliveryTick>, tick: DeliveryTick) {
    if ticks.len() == MAX_TICKS {
        ticks.pop_front();
    }
    ticks.push_back(tick);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_classification_names_outcome_and_spinal_handling() {
        let cases = [
            (
                TickFact::Accepted {
                    decided_by: "adapter".into(),
                },
                TickOutcome::Sent,
                SpinalVerdict::Processed,
                "delivered",
            ),
            (
                TickFact::Accepted {
                    decided_by: "cadence".into(),
                },
                TickOutcome::Sent,
                SpinalVerdict::Skipped,
                "actionable_cadence",
            ),
            (
                TickFact::NotDue,
                TickOutcome::Skipped,
                SpinalVerdict::Skipped,
                "not_due",
            ),
            (
                TickFact::UnchangedContent,
                TickOutcome::Skipped,
                SpinalVerdict::Skipped,
                "unchanged_content",
            ),
            (
                TickFact::Pending,
                TickOutcome::Skipped,
                SpinalVerdict::Processed,
                "pending",
            ),
            (
                TickFact::Suppressed {
                    reason: "no_adapter".into(),
                },
                TickOutcome::Suppressed,
                SpinalVerdict::Skipped,
                "no_adapter",
            ),
            (
                TickFact::Suppressed {
                    reason: "healthy_snapshot".into(),
                },
                TickOutcome::Suppressed,
                SpinalVerdict::Suppressed,
                "healthy_snapshot",
            ),
            (
                TickFact::Suppressed {
                    reason: "model_answer".into(),
                },
                TickOutcome::Suppressed,
                SpinalVerdict::Suppressed,
                "model_answer",
            ),
            (
                TickFact::Suppressed {
                    reason: "in_flight".into(),
                },
                TickOutcome::Skipped,
                SpinalVerdict::Skipped,
                "in_flight",
            ),
            (
                TickFact::Failed,
                TickOutcome::Suppressed,
                SpinalVerdict::Processed,
                "delivery_failed",
            ),
        ];
        for (fact, outcome, spinal, reason) in cases {
            assert_eq!(classify_tick(&fact), (outcome, spinal, reason.into()));
        }
    }
}
