use crate::agent::AgentRole;

use super::{builtins, RoleKindCapability, RoleKindDescriptor};

/// Resolves a `(role, role_profile)` pair to a kind id. A profileless `Manager`
/// is malformed, and a `Worker` subtype can only narrow within leaf capability.
pub fn kind_id(role: Option<AgentRole>, role_profile: Option<&str>) -> Option<&str> {
    let profile = role_profile.map(str::trim).filter(|kind| !kind.is_empty());
    match role? {
        AgentRole::Manager => profile,
        AgentRole::Worker => Some(
            profile
                .filter(is_worker_subtype)
                .unwrap_or(builtins::WORKER),
        ),
    }
}

/// A Worker subtype may not borrow a managing kind's authority. Unknown ids stay
/// as the subtype: they already resolve to leaf capability.
fn is_worker_subtype(kind_id: &&str) -> bool {
    match builtins::lookup(kind_id) {
        Some(descriptor) => {
            !descriptor.manages() && !descriptor.has_team_authority() && !descriptor.anchors_team()
        }
        None => true,
    }
}

/// The descriptor a structural pair resolves to. A Manager's name selects a managing builtin or
/// the one generic manager descriptor; a Worker resolves only to leaf descriptors.
pub fn kind_descriptor(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
) -> Option<&'static RoleKindDescriptor> {
    let kind_id = kind_id(role, role_profile)?;
    registered_descriptor(role?, kind_id)
}

pub fn kind_capability(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
) -> Option<RoleKindCapability> {
    let kind_id = kind_id(role, role_profile)?;
    Some(registered_capability(role?, kind_id))
}

/// The descriptor of a Team registration: its kind and its name decide, never package text.
pub fn registered_descriptor(role: AgentRole, name: &str) -> Option<&'static RoleKindDescriptor> {
    match role {
        AgentRole::Manager => Some(builtins::manager_descriptor(name)),
        AgentRole::Worker => builtins::lookup(name).filter(|descriptor| !descriptor.manages()),
    }
}

/// The capability of a Team registration; a worker name is always leaf.
pub fn registered_capability(role: AgentRole, name: &str) -> RoleKindCapability {
    registered_descriptor(role, name)
        .map(RoleKindDescriptor::capability)
        .unwrap_or(RoleKindCapability::LEAF)
}

/// Display label for a resolved kind. A custom manager carries the generic label and an
/// unknown worker name falls back to the structural role name.
pub fn kind_label(role: Option<AgentRole>, role_profile: Option<&str>) -> Option<&'static str> {
    match kind_descriptor(role, role_profile) {
        Some(descriptor) => Some(descriptor.label()),
        None => role.map(AgentRole::label),
    }
}

pub fn taskless_worker(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    role == Some(AgentRole::Worker)
        && kind_descriptor(role, role_profile).is_some_and(RoleKindDescriptor::is_taskless_worker)
}

pub fn read_only_by_default(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    taskless_worker(role, role_profile)
        && kind_descriptor(role, role_profile)
            .is_some_and(RoleKindDescriptor::is_read_only_by_default)
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

/// Whether this kind is a Manager-kind role rather than a leaf.
pub fn manages(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).manages()
}

pub fn plan_approval(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).plan_approval()
}

pub fn plan_edit(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).plan_edit()
}

pub fn release(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    capability_or_leaf(role, role_profile).release()
}

pub fn can_close_review(role: Option<AgentRole>, role_profile: Option<&str>) -> bool {
    role == Some(AgentRole::Worker) && kind_id(role, role_profile) == Some(builtins::QA)
}
