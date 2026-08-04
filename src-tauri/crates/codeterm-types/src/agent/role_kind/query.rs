use crate::agent::AgentRole;

use super::{builtins, RoleKindCapability, RoleKindDescriptor};

/// Resolves a `(role, role_profile)` pair to a kind id. A profileless `Manager`
/// is malformed, and a `Worker` subtype can only narrow within leaf capability.
pub fn kind_id<'a>(role: Option<AgentRole>, role_profile: Option<&'a str>) -> Option<&'a str> {
    let profile = role_profile.map(str::trim).filter(|kind| !kind.is_empty());
    match role? {
        AgentRole::Manager => profile,
        AgentRole::Worker => Some(profile.filter(is_worker_subtype).unwrap_or(builtins::WORKER)),
    }
}

/// A Worker subtype may not borrow a managing kind's authority. Unknown ids stay
/// as the subtype: they already resolve to leaf capability.
fn is_worker_subtype(kind_id: &&str) -> bool {
    match builtins::lookup(kind_id) {
        Some(descriptor) => !descriptor.has_team_authority() && !descriptor.anchors_team(),
        None => true,
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

/// Display label for a resolved kind. An overlay/unknown kind has no builtin
/// label, so it falls back to the structural role name.
pub fn kind_label(role: Option<AgentRole>, role_profile: Option<&str>) -> Option<&'static str> {
    match kind_descriptor(role, role_profile) {
        Some(descriptor) => Some(descriptor.label()),
        None => role.map(AgentRole::label),
    }
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

/// Whether this kind may rename, adopt into, or close the Team it belongs to.
pub fn has_team_authority(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).has_team_authority()
}
