use codeterm_team_package::is_role_name;

use super::builtins;
use crate::agent::AgentRole;

/// A role type named by `--role`: a manager slot, the host-only watcher, or a worker name.
/// The structural role always derives from the type, never from the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleType {
    pub role: AgentRole,
    pub kind_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleTypeError {
    Empty,
    /// A structural role literal such as `Manager` or `Worker`; `--role` names a type.
    StructuralLiteral(String),
    /// A kind the host owns and never launches by name.
    HostOwned(String),
    Malformed(String),
    /// A worker name this Team neither registers nor ships as a builtin.
    UnavailableWorker(String),
}

impl std::fmt::Display for RoleTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "--role needs a role type"),
            Self::StructuralLiteral(value) => write!(
                f,
                "`{value}` is a structural role, not a role type; name a manager slot ({}), watcher, or a worker type such as `worker`",
                MANAGER_TYPES.join("|")
            ),
            Self::HostOwned(value) => write!(f, "`{value}` is owned by the host and cannot be launched by role"),
            Self::Malformed(value) => write!(
                f,
                "`{value}` is not a role type: use lowercase letters, digits and `_`, starting with a letter"
            ),
            Self::UnavailableWorker(value) => write!(
                f,
                "worker type `{value}` is not available to this Team; see `domios role list --team <team-id>`"
            ),
        }
    }
}

impl std::error::Error for RoleTypeError {}

pub const MANAGER_TYPES: [&str; 4] = [
    builtins::ORCHESTRATOR,
    builtins::PLANNER,
    builtins::CARTOGRAPHER,
    builtins::CURATOR,
];

const STRUCTURAL_LITERALS: [&str; 6] = [
    "Manager",
    "Worker",
    "Orchestrator",
    "Conductor",
    "Planner",
    "Watcher",
];

impl RoleType {
    /// Parses the type; worker availability is the Team registry's to judge.
    pub fn parse(value: &str) -> Result<Self, RoleTypeError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(RoleTypeError::Empty);
        }
        if STRUCTURAL_LITERALS.contains(&value) {
            return Err(RoleTypeError::StructuralLiteral(value.to_string()));
        }
        if value == builtins::GENERAL_AGENT {
            return Err(RoleTypeError::HostOwned(value.to_string()));
        }
        if !is_role_name(value) {
            return Err(RoleTypeError::Malformed(value.to_string()));
        }
        let role = if MANAGER_TYPES.contains(&value) || value == builtins::WATCHER {
            AgentRole::Manager
        } else {
            AgentRole::Worker
        };
        Ok(Self {
            role,
            kind_id: value.to_string(),
        })
    }

    pub fn is_worker(&self) -> bool {
        self.role == AgentRole::Worker
    }

    /// The persisted `(role, role_profile)` pair this type launches as.
    pub fn wire_pair(&self) -> (&'static str, &str) {
        (self.role.label(), &self.kind_id)
    }
}

#[cfg(test)]
#[path = "role_type/tests.rs"]
mod tests;
