use codeterm_team_package::is_role_name;

use super::builtins;
use crate::agent::AgentRole;

/// A role name given to `--role`, checked for syntax only. Its kind comes from the Team's
/// registration; only the host-owned watcher has a kind without one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleName(String);

/// A role resolved against a Team: the registration's kind gives the structural role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleType {
    pub role: AgentRole,
    pub kind_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleTypeError {
    Empty,
    /// A structural role literal such as `Manager` or `Worker`; `--role` names a role.
    StructuralLiteral(String),
    /// A kind the host owns and never launches by name.
    HostOwned(String),
    Malformed(String),
    /// The Team lists no role of this name; `roles` are the names it does list.
    NotRegistered { name: String, roles: Vec<String> },
    /// Dispatch launches workers; this name is one of the Team's managers.
    NotAWorker { name: String, workers: Vec<String> },
    /// Dispatch without `--role` uses `worker`, which this Team does not list.
    NoDefaultWorker { workers: Vec<String> },
}

fn names(list: &[String]) -> String {
    if list.is_empty() {
        "none".to_string()
    } else {
        list.join(", ")
    }
}

impl std::fmt::Display for RoleTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "--role needs a role name"),
            Self::StructuralLiteral(value) => write!(
                f,
                "`{value}` is a structural role, not a role name; name a role the Team lists or watcher"
            ),
            Self::HostOwned(value) => write!(f, "`{value}` is owned by the host and cannot be launched by role"),
            Self::Malformed(value) => write!(
                f,
                "`{value}` is not a role name: use lowercase letters, digits and `_`, starting with a letter"
            ),
            Self::NotRegistered { name, roles } => write!(
                f,
                "this Team has no role `{name}`; its roles are: {}",
                names(roles)
            ),
            Self::NotAWorker { name, workers } => write!(
                f,
                "`{name}` is a manager of this Team; start it with agent spawn --role {name}. Its workers are: {}",
                names(workers)
            ),
            Self::NoDefaultWorker { workers } => write!(
                f,
                "this Team lists no `worker` role; pass --role with one of its workers: {}",
                names(workers)
            ),
        }
    }
}

impl std::error::Error for RoleTypeError {}

const STRUCTURAL_LITERALS: [&str; 6] = [
    "Manager",
    "Worker",
    "Orchestrator",
    "Conductor",
    "Planner",
    "Watcher",
];

impl RoleName {
    /// Checks the name's syntax; whether the Team lists it is the registry's to judge.
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
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The host-only watcher is a manager without any Team registration.
    pub fn host_owned(&self) -> Option<RoleType> {
        (self.0 == builtins::WATCHER).then(|| RoleType::new(AgentRole::Manager, &self.0))
    }

    /// The spec reserves the built-in manager names (and the host the watcher) for managers,
    /// so these names can never be workers; any other name is decided by the Team.
    pub fn is_manager_name(&self) -> bool {
        builtins::is_builtin_manager(&self.0) || self.0 == builtins::WATCHER
    }
}

impl std::fmt::Display for RoleName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl RoleType {
    pub fn new(role: AgentRole, kind_id: &str) -> Self {
        Self {
            role,
            kind_id: kind_id.to_string(),
        }
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
