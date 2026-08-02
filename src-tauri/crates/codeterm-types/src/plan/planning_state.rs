use serde::{Deserialize, Serialize};

/// Planner-owned progression of a plan's structure. This is the single gate for
/// execution dispatch: only `Approved` unlocks draft execution tasks.
/// `PlanMode` remains the lifecycle (planning/approved/execution/complete);
/// `planning_state` is the finer-grained planner progress that gates approval.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanningState {
    #[default]
    Drafting,
    StructureReady,
    Approved,
}

impl PlanningState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanningState::Drafting => "drafting",
            PlanningState::StructureReady => "structure_ready",
            PlanningState::Approved => "approved",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "drafting" | "draft" => Some(PlanningState::Drafting),
            "structure_ready" | "structure-ready" | "ready" => Some(PlanningState::StructureReady),
            "approved" | "approve" => Some(PlanningState::Approved),
            _ => None,
        }
    }

    /// Execution dispatch is unlocked only once the Orchestrator approves.
    pub fn allows_execution_tasks(&self) -> bool {
        matches!(self, PlanningState::Approved)
    }

    /// Human label for headers/chips.
    pub fn label(&self) -> &'static str {
        match self {
            PlanningState::Drafting => "Drafting",
            PlanningState::StructureReady => "Structure ready",
            PlanningState::Approved => "Approved",
        }
    }
}

/// Who advanced the planning state. Only a planner tab or the user is a legal
/// author — the state never advances by generic tab guessing or orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanningAuthor {
    Planner { tab_id: String },
    User,
}

impl PlanningAuthor {
    pub fn label(&self) -> String {
        match self {
            PlanningAuthor::Planner { tab_id } => format!("planner {tab_id}"),
            PlanningAuthor::User => "user".into(),
        }
    }
}

/// An append-only working-point record of a planning-state advance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanningTransition {
    pub from: PlanningState,
    pub to: PlanningState,
    pub by: PlanningAuthor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_approved_unlocks_execution_tasks() {
        assert!(!PlanningState::Drafting.allows_execution_tasks());
        assert!(!PlanningState::StructureReady.allows_execution_tasks());
        assert!(PlanningState::Approved.allows_execution_tasks());
    }

    #[test]
    fn parse_and_as_str_roundtrip() {
        for s in [
            PlanningState::Drafting,
            PlanningState::StructureReady,
            PlanningState::Approved,
        ] {
            assert_eq!(PlanningState::parse(s.as_str()), Some(s));
        }
        assert_eq!(PlanningState::parse("nonsense"), None);
        assert_eq!(
            PlanningState::parse("STRUCTURE-READY"),
            Some(PlanningState::StructureReady)
        );
    }

    #[test]
    fn default_is_drafting() {
        assert_eq!(PlanningState::default(), PlanningState::Drafting);
    }

    #[test]
    fn transition_serde_roundtrips() {
        let t = PlanningTransition {
            from: PlanningState::Drafting,
            to: PlanningState::Approved,
            by: PlanningAuthor::Planner {
                tab_id: "tab_x".into(),
            },
            note: Some("looks ready".into()),
            at: 42,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"kind\":\"planner\""));
        let back: PlanningTransition = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
