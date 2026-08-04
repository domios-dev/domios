use serde::{Deserialize, Serialize};

/// The two structural agent roles. Every finer-grained kind lives in
/// `role_profile` and resolves through `role_kind` descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    Manager,
    Worker,
}

/// A `(role, role_profile)` pair recovered from a persisted record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredRole {
    pub role: AgentRole,
    pub role_profile: Option<String>,
}

impl RestoredRole {
    fn new(role: AgentRole, role_profile: Option<&str>) -> Self {
        Self {
            role,
            role_profile: role_profile.map(str::to_string),
        }
    }
}

impl AgentRole {
    /// Accepts the shipped literals plus the two legacy manager spellings.
    pub fn from_str_compat(s: &str) -> Option<Self> {
        match s {
            "Orchestrator" | "Conductor" | "Manager" => Some(Self::Manager),
            "Worker" => Some(Self::Worker),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Manager => "Manager",
            Self::Worker => "Worker",
        }
    }
}

/// Literal legacy migration for a persisted `(role, role_profile)` pair. Legacy
/// `Orchestrator`/`Conductor` rows carried no profile; a kindless `Manager` has no anchor to inherit.
pub fn restore_role(role: Option<&str>, role_profile: Option<&str>) -> Option<RestoredRole> {
    let profile = role_profile
        .map(str::trim)
        .filter(|profile| !profile.is_empty());
    match role.map(str::trim)? {
        "Orchestrator" | "Conductor" => Some(RestoredRole::new(
            AgentRole::Manager,
            Some(super::role_kind::builtins::ORCHESTRATOR),
        )),
        "Manager" => profile.map(|profile| RestoredRole::new(AgentRole::Manager, Some(profile))),
        "Worker" => Some(RestoredRole::new(AgentRole::Worker, profile)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Done,
    InReview,
    Blocked,
    Partial,
}

impl ReportStatus {
    pub fn label(self) -> &'static str {
        match self {
            ReportStatus::Done => "Done",
            ReportStatus::InReview => "In Review",
            ReportStatus::Blocked => "Blocked",
            ReportStatus::Partial => "Partial",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restored(role: &str, profile: Option<&str>) -> Option<(AgentRole, Option<String>)> {
        restore_role(Some(role), profile).map(|restored| (restored.role, restored.role_profile))
    }

    #[test]
    fn legacy_orchestrator_literal_restores_the_orchestrator_kind() {
        assert_eq!(
            restored("Orchestrator", None),
            Some((AgentRole::Manager, Some("orchestrator".to_string())))
        );
    }

    #[test]
    fn legacy_conductor_literal_restores_the_orchestrator_kind() {
        assert_eq!(
            restored("Conductor", None),
            Some((AgentRole::Manager, Some("orchestrator".to_string())))
        );
    }

    #[test]
    fn legacy_orchestrator_row_ignores_a_stored_profile() {
        assert_eq!(
            restored("Orchestrator", Some("planner")),
            Some((AgentRole::Manager, Some("orchestrator".to_string()))),
            "a legacy Orchestrator row never carried a meaningful profile"
        );
    }

    #[test]
    fn legacy_manager_pairs_round_trip_their_profile() {
        assert_eq!(
            restored("Manager", Some("planner")),
            Some((AgentRole::Manager, Some("planner".to_string())))
        );
        assert_eq!(
            restored("Manager", Some("watcher")),
            Some((AgentRole::Manager, Some("watcher".to_string())))
        );
        assert_eq!(
            restored("Manager", Some("general_agent")),
            Some((AgentRole::Manager, Some("general_agent".to_string())))
        );
    }

    #[test]
    fn profileless_manager_restores_roleless_instead_of_anchoring() {
        assert_eq!(
            restored("Manager", None),
            None,
            "a kindless Manager must not inherit anchoring authority"
        );
    }

    #[test]
    fn worker_pairs_keep_their_subtype() {
        assert_eq!(restored("Worker", None), Some((AgentRole::Worker, None)));
        assert_eq!(
            restored("Worker", Some("rust-platform-dev")),
            Some((AgentRole::Worker, Some("rust-platform-dev".to_string())))
        );
    }

    #[test]
    fn unknown_and_blank_literals_restore_roleless() {
        assert_eq!(restored("Overlord", None), None);
        assert_eq!(restore_role(None, Some("planner")), None);
        assert_eq!(restored("Manager", Some("   ")), None);
    }

    #[test]
    fn from_str_compat_folds_both_legacy_manager_spellings() {
        assert_eq!(
            AgentRole::from_str_compat("Orchestrator"),
            Some(AgentRole::Manager)
        );
        assert_eq!(
            AgentRole::from_str_compat("Conductor"),
            Some(AgentRole::Manager)
        );
        assert_eq!(
            AgentRole::from_str_compat("Manager"),
            Some(AgentRole::Manager)
        );
        assert_eq!(AgentRole::from_str_compat("Worker"), Some(AgentRole::Worker));
        assert_eq!(AgentRole::from_str_compat("Planner"), None);
    }
}
