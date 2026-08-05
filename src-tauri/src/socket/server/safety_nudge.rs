use serde_json::{json, Value};

pub(super) const NUDGE_LANE_BRANCH_UNRECORDED: &str = "lane.branch_unrecorded";
pub(super) const NUDGE_LANE_BASE_UNRECORDED: &str = "lane.base_unrecorded";
pub(super) const NUDGE_TEAM_IDENTITY_UNRECORDED: &str = "lane.team_identity_unrecorded";
pub(super) const NUDGE_MERGE_DESTINATION_UNRECORDED: &str = "lane.merge_destination_unrecorded";
pub(super) const NUDGE_RETROSPECTIVE_UNRECORDED: &str = "plan.retrospective_unrecorded";

pub(super) const NUDGE_IDS: &[&str] = &[
    NUDGE_LANE_BRANCH_UNRECORDED,
    NUDGE_LANE_BASE_UNRECORDED,
    NUDGE_TEAM_IDENTITY_UNRECORDED,
    NUDGE_MERGE_DESTINATION_UNRECORDED,
    NUDGE_RETROSPECTIVE_UNRECORDED,
];

pub(super) const SAFETY_INCIDENT_KIND: &str = "incident";

/// Advisory by default: a stable id, why it matters, and the exact fix — carried
/// on a response that still succeeded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SafetyNudge {
    pub id: &'static str,
    pub reason: String,
    pub safe_alternative: String,
}

impl SafetyNudge {
    pub(super) fn new(id: &'static str, reason: String, safe_alternative: String) -> Self {
        debug_assert!(NUDGE_IDS.contains(&id), "unregistered nudge id: {id}");
        Self {
            id,
            reason,
            safe_alternative,
        }
    }

    pub(super) fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "reason": self.reason,
            "safe_alternative": self.safe_alternative,
        })
    }

    pub(super) fn to_hint(&self) -> String {
        format!(
            "nudge [{}]: {} Safe alternative: {}",
            self.id, self.reason, self.safe_alternative
        )
    }
}

pub(super) fn safety_incident_text(subject: &str, violation_id: &str, reason: &str) -> String {
    format!("safety {violation_id} on {subject}: {reason}")
}

/// Advisory findings never change the outcome — they ride alongside a successful
/// result as structured ids plus the `_hints` the CLI already prints.
pub(super) fn apply_safety_nudges(result: &mut Value, nudges: &[SafetyNudge]) {
    if nudges.is_empty() {
        return;
    }
    result["safety_nudges"] = json!(nudges.iter().map(SafetyNudge::to_json).collect::<Vec<_>>());
    // Merge, never overwrite: a caller may already have pushed its own hints.
    let mut hints: Vec<String> = result["_hints"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    hints.extend(nudges.iter().map(SafetyNudge::to_hint));
    result["_hints"] = json!(hints);
}

#[cfg(test)]
mod tests;
