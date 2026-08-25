//! The refusal value behind "the General Agent coordinates Teams, it never
//! joins one". Lives beside [`super::Team`] so daemon and CLI share one id.

use serde::Serialize;
use serde_json::json;

/// Stable id for this refusal. Every transport forwards it verbatim, so the
/// REST, Tauri, socket and CLI callers all report the same violation.
pub const GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION: &str = "general_agent.team_membership_forbidden";

const WORKSPACE_PLACEHOLDER: &str = "<project-workspace-id>";
const PROVIDER_PLACEHOLDER: &str = "<provider>";

/// What the refused mutation was about to do with the General Agent's tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipAttempt {
    /// Bind a Team onto the tab.
    Join,
    /// Read the tab's own Team membership as authority for a Team mutation.
    ExerciseMembership,
}

impl MembershipAttempt {
    fn phrasing(self) -> &'static str {
        match self {
            Self::Join => "be a member of a Team",
            Self::ExerciseMembership => "act with Team membership authority",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::ExerciseMembership => "exercise_membership",
        }
    }
}

/// Placeholders, not shell prompts: the same literal has to be runnable once
/// substituted in either shell. Real values are used wherever the refused call
/// already carried them, so the example is runnable rather than illustrative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorrectiveShape {
    workspace_id: Option<String>,
    orchestrator_agent: Option<String>,
    planner_agent: Option<String>,
}

fn present(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

impl CorrectiveShape {
    pub fn in_workspace(workspace_id: Option<&str>) -> Self {
        Self {
            workspace_id: present(workspace_id),
            ..Self::default()
        }
    }

    pub fn with_agents(mut self, orchestrator: Option<&str>, planner: Option<&str>) -> Self {
        self.orchestrator_agent = present(orchestrator);
        self.planner_agent = present(planner);
        self
    }

    fn workspace(&self) -> &str {
        self.workspace_id
            .as_deref()
            .unwrap_or(WORKSPACE_PLACEHOLDER)
    }

    fn orchestrator(&self) -> &str {
        self.orchestrator_agent
            .as_deref()
            .unwrap_or(PROVIDER_PLACEHOLDER)
    }

    fn planner(&self) -> &str {
        self.planner_agent
            .as_deref()
            .or(self.orchestrator_agent.as_deref())
            .unwrap_or(PROVIDER_PLACEHOLDER)
    }

    /// The whole correct shape in one action: an Orchestrator + Planner pair
    /// that belongs to a project workspace, not to the General Agent.
    pub fn pair_command(&self) -> String {
        format!(
            "codeterm team pair --orchestrator {} --planner {} --workspace {}",
            self.orchestrator(),
            self.planner(),
            self.workspace()
        )
    }

    /// The two-step form, for when the Orchestrator is spawned first.
    pub fn spawn_orchestrator_command(&self) -> String {
        format!(
            "codeterm agent spawn {} --role Orchestrator --workspace {} --task \"<objective>\"",
            self.orchestrator(),
            self.workspace()
        )
    }
}

/// A refused Team membership mutation: who asked, why it is refused, and the
/// command that does the legitimate thing instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneralAgentTeamRefusal {
    pub violation_id: &'static str,
    pub tab_id: String,
    pub role_kind_id: String,
    pub attempt: MembershipAttempt,
    pub shape: CorrectiveShape,
}

impl GeneralAgentTeamRefusal {
    pub fn new(
        tab_id: impl Into<String>,
        role_kind_id: impl Into<String>,
        attempt: MembershipAttempt,
        shape: CorrectiveShape,
    ) -> Self {
        Self {
            violation_id: GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION,
            tab_id: tab_id.into(),
            role_kind_id: role_kind_id.into(),
            attempt,
            shape,
        }
    }

    pub fn reason(&self) -> String {
        format!(
            "Tab {} is the General Agent (role kind `{}`) and cannot {}. \
             The General Agent creates and coordinates Teams for OTHER agents and is never a \
             member of one. No Team was changed.",
            self.tab_id,
            self.role_kind_id,
            self.attempt.phrasing()
        )
    }

    /// The one command that reaches the intended outcome legitimately.
    pub fn corrective_command(&self) -> String {
        self.shape.pair_command()
    }

    pub fn data(&self) -> serde_json::Value {
        json!({
            "type": "general_agent_team_membership",
            "stage": "team_membership_guard",
            "violation_id": self.violation_id,
            "reason": self.reason(),
            "tab_id": self.tab_id,
            "role_kind_id": self.role_kind_id,
            "attempt": self.attempt,
            "attempt_id": self.attempt.id(),
            "corrective_command": self.corrective_command(),
            "spawn_orchestrator_command": self.shape.spawn_orchestrator_command(),
        })
    }
}

impl std::fmt::Display for GeneralAgentTeamRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} Create the Team for an Orchestrator + Planner pair in a project workspace \
             instead:\n  {}",
            self.violation_id,
            self.reason(),
            self.corrective_command()
        )
    }
}

impl From<GeneralAgentTeamRefusal> for String {
    fn from(refusal: GeneralAgentTeamRefusal) -> Self {
        refusal.to_string()
    }
}

#[cfg(test)]
mod tests;
