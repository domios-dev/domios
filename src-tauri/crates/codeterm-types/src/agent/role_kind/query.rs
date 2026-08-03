use crate::agent::AgentRole;

use super::{builtins, RoleKindCapability, RoleKindDescriptor};

/// Resolves a `(role, role_profile)` pair to a kind id.
///
/// A `Manager` with no profile is malformed, not a default: it resolves to
/// `None` rather than silently inheriting anchoring authority.
pub fn kind_id<'a>(role: Option<AgentRole>, role_profile: Option<&'a str>) -> Option<&'a str> {
    let profile = role_profile.map(str::trim).filter(|kind| !kind.is_empty());
    match role? {
        AgentRole::Orchestrator => Some(builtins::ORCHESTRATOR),
        AgentRole::Worker => Some(builtins::WORKER),
        AgentRole::Manager => profile,
    }
}

pub fn kind_descriptor(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
) -> Option<&'static RoleKindDescriptor> {
    builtins::lookup(kind_id(role, role_profile)?)
}

pub fn kind_capability(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
) -> Option<RoleKindCapability> {
    kind_id(role, role_profile).map(builtins::capability_for)
}

fn capability_or_leaf(role: Option<AgentRole>, role_profile: Option<&str>) -> RoleKindCapability {
    kind_capability(role, role_profile).unwrap_or(RoleKindCapability::LEAF)
}

pub fn anchors_team(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).anchors_team()
}

pub fn requires_parent(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).requires_parent()
}

pub fn can_spawn(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).can_spawn()
}
