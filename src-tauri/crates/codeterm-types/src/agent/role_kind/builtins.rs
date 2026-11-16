use super::{RoleKindCapability, RoleKindDescriptor};

pub use codeterm_team_package::{is_builtin_manager, BUILTIN_MANAGERS};

pub const ORCHESTRATOR: &str = "orchestrator";
pub const GENERAL_AGENT: &str = "general_agent";
pub const PLANNER: &str = "planner";
pub const WATCHER: &str = "watcher";
pub const WORKER: &str = "worker";
pub const QA: &str = "qa";
pub const RELEASE_MANAGER: &str = "release_manager";
pub const REVIEWER: &str = "reviewer";
/// Manager-kind leaf that authors the essence map. It is non-anchoring and
/// carries no spawn, plan, release or team-staffing authority.
pub const CARTOGRAPHER: &str = "cartographer";
pub const CURATOR: &str = "curator";
/// The id of the one host-owned descriptor every other manager name resolves to.
pub const CUSTOM_MANAGER: &str = "custom_manager";

// Embedded in the binary and non-overridable: a disk overlay able to rewrite
// these rows could mint a Team-anchoring, plan-owning agent.
static BUILTINS: [RoleKindDescriptor; 10] = [
    RoleKindDescriptor::anchoring(ORCHESTRATOR, "Orchestrator"),
    RoleKindDescriptor::anchoring(GENERAL_AGENT, "General Agent"),
    RoleKindDescriptor::managing_leaf(PLANNER, "Planner"),
    RoleKindDescriptor::managing_leaf_observer(WATCHER, "Watcher"),
    RoleKindDescriptor::leaf(WORKER, "Worker"),
    RoleKindDescriptor::taskless_worker(QA, "QA"),
    RoleKindDescriptor::taskless_worker(RELEASE_MANAGER, "Release Manager"),
    RoleKindDescriptor::read_only_taskless_worker(REVIEWER, "Reviewer"),
    RoleKindDescriptor::managing_leaf_withheld(CARTOGRAPHER, "Cartographer"),
    RoleKindDescriptor::managing_leaf_withheld(CURATOR, "Curator"),
];

/// A package manager without a built-in name: the Cartographer's limits (a managing leaf with
/// no spawn, team authority, plan approval or edit, task acceptance or release), owned by the host.
static CUSTOM_MANAGER_KIND: RoleKindDescriptor =
    RoleKindDescriptor::generic_manager(CUSTOM_MANAGER, "Manager");

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

/// A Domios worker kind (`qa`, `reviewer`, `release_manager`): never a manager name.
pub fn is_taskless_worker_kind(kind_id: &str) -> bool {
    lookup(kind_id).is_some_and(RoleKindDescriptor::is_taskless_worker)
}

/// An unknown kind id resolves to leaf capability, never to anchoring.
pub fn capability_for(kind_id: &str) -> RoleKindCapability {
    match lookup(kind_id) {
        Some(descriptor) => descriptor.capability(),
        None => RoleKindCapability::LEAF,
    }
}

/// The host-owned descriptor of every custom manager.
pub fn custom_manager() -> &'static RoleKindDescriptor {
    &CUSTOM_MANAGER_KIND
}

/// The descriptor a manager named `name` resolves to: a managing builtin keeps its own, and
/// every other name, a builtin worker name included, gets the one generic manager descriptor.
pub fn manager_descriptor(name: &str) -> &'static RoleKindDescriptor {
    match lookup(name) {
        Some(descriptor) if descriptor.manages() => descriptor,
        _ => custom_manager(),
    }
}
