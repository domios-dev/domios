//! Deterministic per-action authorization for a recovered seat's approval
//! surface (#33). On a seat recovered after a crash a blocked command showed
//! no approval control, and identical actions were authorized differently
//! depending on whether they were sent singly or batched. Here authorization
//! is a pure function of the action alone: batching an already-permitted action
//! with another never changes its verdict, and a blocked batch names the exact
//! action that needs approval instead of misreporting the whole request.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    /// Read-only inspection (file hash, screen read). Auto-granted.
    ReadOnly,
    /// Focus/navigation with no state change. Auto-granted.
    Focus,
    /// Mutates state — always requires explicit approval.
    StateChange,
}

impl ActionClass {
    fn auto_granted(self) -> bool {
        matches!(self, ActionClass::ReadOnly | ActionClass::Focus)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatAction {
    pub id: String,
    pub label: String,
    pub class: ActionClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Granted,
    NeedsApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionVerdict {
    pub action_id: String,
    pub decision: ApprovalDecision,
}

/// Authorize one action from its class alone — never from the request it rode in.
pub fn evaluate_action(action: &SeatAction) -> ActionVerdict {
    ActionVerdict {
        action_id: action.id.clone(),
        decision: if action.class.auto_granted() {
            ApprovalDecision::Granted
        } else {
            ApprovalDecision::NeedsApproval
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchVerdict {
    pub granted: bool,
    /// Per-action verdicts — always equal to evaluating each action singly.
    pub actions: Vec<ActionVerdict>,
    /// The first action that blocks the batch, named precisely. `None` when the
    /// whole batch is granted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_action: Option<ActionVerdict>,
}

/// Batch authorization is the pointwise per-action authorization: the request
/// is granted iff every action is, and a blocked request names the first action
/// needing approval rather than reporting the whole batch as denied.
pub fn evaluate_batch(actions: &[SeatAction]) -> BatchVerdict {
    let verdicts: Vec<ActionVerdict> = actions.iter().map(evaluate_action).collect();
    let blocked_action = verdicts
        .iter()
        .find(|v| v.decision == ApprovalDecision::NeedsApproval)
        .cloned();
    BatchVerdict {
        granted: blocked_action.is_none(),
        actions: verdicts,
        blocked_action,
    }
}

/// The approval control a recovered seat must render whenever a command blocks
/// on permission — names the exact blocked action so it is reachable and
/// meaningful in both the visual and accessibility trees. `None` when nothing
/// in the request needs approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSurface {
    pub tab_id: String,
    pub blocked_action_id: String,
    pub blocked_action_label: String,
    /// Semantic decisions the operator can take — not raw PTY keystrokes.
    pub decisions: Vec<ApprovalDecisionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecisionOption {
    pub value: String,
    pub label: String,
}

pub fn recovered_seat_surface(tab_id: &str, actions: &[SeatAction]) -> Option<ApprovalSurface> {
    let blocked = evaluate_batch(actions).blocked_action?;
    let label = actions
        .iter()
        .find(|a| a.id == blocked.action_id)
        .map(|a| a.label.clone())
        .unwrap_or_default();
    Some(ApprovalSurface {
        tab_id: tab_id.to_string(),
        blocked_action_id: blocked.action_id,
        blocked_action_label: label,
        decisions: vec![
            ApprovalDecisionOption {
                value: "grant".into(),
                label: "Grant".into(),
            },
            ApprovalDecisionOption {
                value: "deny".into(),
                label: "Deny".into(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str, class: ActionClass) -> SeatAction {
        SeatAction {
            id: id.to_string(),
            label: format!("action {id}"),
            class,
        }
    }

    /// The issue's acceptance test: the same permitted focus/read sequence run
    /// once as individual actions and once as a single batch yields identical
    /// per-action authorization.
    #[test]
    fn single_and_batched_authorization_agree() {
        let actions = vec![
            action("focus", ActionClass::Focus),
            action("read", ActionClass::ReadOnly),
        ];
        let batch = evaluate_batch(&actions);
        let singles: Vec<ActionVerdict> = actions.iter().map(evaluate_action).collect();
        assert_eq!(batch.actions, singles, "batching must not change verdicts");
        assert!(batch.granted);
        assert!(batch.blocked_action.is_none());
    }

    #[test]
    fn batching_a_state_change_names_the_exact_blocked_action() {
        let actions = vec![
            action("focus", ActionClass::Focus),
            action("read", ActionClass::ReadOnly),
            action("write", ActionClass::StateChange),
        ];
        let batch = evaluate_batch(&actions);
        assert!(!batch.granted);
        let blocked = batch.blocked_action.expect("a blocked action must be named");
        assert_eq!(blocked.action_id, "write");
        assert_eq!(blocked.decision, ApprovalDecision::NeedsApproval);
        // The permitted actions keep their single-action grant even in the batch.
        assert_eq!(batch.actions[0].decision, ApprovalDecision::Granted);
        assert_eq!(batch.actions[1].decision, ApprovalDecision::Granted);
    }

    #[test]
    fn a_permitted_action_stays_permitted_when_combined() {
        let alone = evaluate_batch(&[action("read", ActionClass::ReadOnly)]);
        let combined = evaluate_batch(&[
            action("read", ActionClass::ReadOnly),
            action("write", ActionClass::StateChange),
        ]);
        assert_eq!(
            alone.actions[0].decision, combined.actions[0].decision,
            "read stays granted whether alone or batched with a state change"
        );
        assert!(alone.granted);
        assert!(!combined.granted);
    }

    #[test]
    fn recovered_seat_renders_an_approval_control_for_the_blocked_action() {
        let actions = vec![
            action("focus", ActionClass::Focus),
            action("write", ActionClass::StateChange),
        ];
        let surface =
            recovered_seat_surface("seat-1", &actions).expect("blocked batch must render a surface");
        assert_eq!(surface.tab_id, "seat-1");
        assert_eq!(surface.blocked_action_id, "write");
        assert_eq!(surface.blocked_action_label, "action write");
        let values: Vec<&str> = surface.decisions.iter().map(|d| d.value.as_str()).collect();
        assert_eq!(values, vec!["grant", "deny"]);
    }

    #[test]
    fn fully_permitted_request_needs_no_surface() {
        let actions = vec![
            action("focus", ActionClass::Focus),
            action("read", ActionClass::ReadOnly),
        ];
        assert!(recovered_seat_surface("seat-1", &actions).is_none());
    }
}
