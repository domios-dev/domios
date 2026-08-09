use super::{RoleKindCapability, RoleKindDescriptor};

pub const ORCHESTRATOR: &str = "orchestrator";
pub const GENERAL_AGENT: &str = "general_agent";
pub const PLANNER: &str = "planner";
pub const WATCHER: &str = "watcher";
pub const WORKER: &str = "worker";

// Embedded in the binary and non-overridable: a disk overlay able to rewrite
// these rows could mint a Team-anchoring, plan-owning agent.
static BUILTINS: [RoleKindDescriptor; 5] = [
    RoleKindDescriptor::anchoring(ORCHESTRATOR, "Orchestrator"),
    RoleKindDescriptor::anchoring(GENERAL_AGENT, "General Agent"),
    RoleKindDescriptor::managing_leaf(PLANNER, "Planner"),
    RoleKindDescriptor::managing_leaf(WATCHER, "Watcher"),
    RoleKindDescriptor::leaf(WORKER, "Worker"),
];

pub fn all() -> &'static [RoleKindDescriptor] {
    &BUILTINS
}

pub fn lookup(kind_id: &str) -> Option<&'static RoleKindDescriptor> {
    BUILTINS
        .iter()
        .find(|descriptor| descriptor.id() == kind_id)
}

pub fn is_builtin(kind_id: &str) -> bool {
    lookup(kind_id).is_some()
}

/// An unknown kind id resolves to leaf capability, never to anchoring.
pub fn capability_for(kind_id: &str) -> RoleKindCapability {
    match lookup(kind_id) {
        Some(descriptor) => descriptor.capability(),
        None => RoleKindCapability::LEAF,
    }
}
