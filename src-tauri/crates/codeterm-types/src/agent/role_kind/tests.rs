use crate::agent::AgentRole;

use super::builtins::{self, GENERAL_AGENT, ORCHESTRATOR, PLANNER, WATCHER, WORKER};
use super::query;
use super::{RoleKindCapability, RoleKindDescriptor, RoleKindOrigin};

struct Row {
    id: &'static str,
    anchors_team: bool,
    requires_parent: bool,
    can_spawn: bool,
}

impl Row {
    fn caps(&self) -> (bool, bool, bool) {
        (self.anchors_team, self.requires_parent, self.can_spawn)
    }
}

const ROWS: &[Row] = &[
    Row {
        id: ORCHESTRATOR,
        anchors_team: true,
        requires_parent: false,
        can_spawn: true,
    },
    Row {
        id: GENERAL_AGENT,
        anchors_team: true,
        requires_parent: false,
        can_spawn: true,
    },
    Row {
        id: PLANNER,
        anchors_team: false,
        requires_parent: true,
        can_spawn: false,
    },
    Row {
        id: WATCHER,
        anchors_team: false,
        requires_parent: true,
        can_spawn: false,
    },
    Row {
        id: WORKER,
        anchors_team: false,
        requires_parent: true,
        can_spawn: false,
    },
];

const BLANK_PROFILES: &[Option<&str>] = &[None, Some(""), Some("   ")];

const ANY_PROFILES: &[Option<&str>] = &[
    None,
    Some(""),
    Some("   "),
    Some("planner"),
    Some("orchestrator"),
    Some("mystery"),
];

const UNKNOWN_KIND_IDS: &[&str] = &[
    "reviewer",
    "Orchestrator",
    "general-agent",
    "orchestrator ",
    "worker:gpu",
    "",
];

const UNKNOWN_PROFILES: &[&str] = &[
    "reviewer",
    "Orchestrator",
    "general-agent",
    "worker:gpu",
    "planner_v2",
];

const OVERLAY_IDS: &[&str] = &[
    ORCHESTRATOR,
    GENERAL_AGENT,
    PLANNER,
    "reviewer",
    "worker_gpu",
    "../../escape",
    "",
];

fn caps(capability: RoleKindCapability) -> (bool, bool, bool) {
    let anchors_team = capability.anchors_team();
    let requires_parent = capability.requires_parent();
    (anchors_team, requires_parent, capability.can_spawn())
}

#[test]
fn every_builtin_matches_its_declared_capability_row() {
    for row in ROWS {
        let id = row.id;
        let descriptor = builtins::lookup(id).expect("builtin kind");
        assert_eq!(caps(descriptor.capability()), row.caps(), "{id}");
        assert_eq!(descriptor.origin(), RoleKindOrigin::Builtin, "{id}");
        assert!(descriptor.is_builtin(), "{id}");
        assert!(!descriptor.label().is_empty(), "{id}");
    }
}

#[test]
fn builtin_table_is_exactly_the_declared_rows() {
    let ids: Vec<&str> = builtins::all().iter().map(RoleKindDescriptor::id).collect();
    let expected: Vec<&str> = ROWS.iter().map(|row| row.id).collect();
    assert_eq!(ids, expected);

    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "duplicate builtin kind id");
}

#[test]
fn orchestrator_resolves_to_the_orchestrator_kind_for_any_profile() {
    let role = Some(AgentRole::Orchestrator);
    for profile in ANY_PROFILES.iter().copied() {
        assert_eq!(query::kind_id(role, profile), Some(ORCHESTRATOR));
        assert!(query::anchors_team(role, profile));
        assert!(query::can_spawn(role, profile));
        assert!(!query::requires_parent(role, profile));
    }
}

#[test]
fn manager_resolves_the_kind_named_by_its_profile() {
    let role = Some(AgentRole::Manager);
    assert_eq!(query::kind_id(role, Some("planner")), Some(PLANNER));
    assert_eq!(query::kind_id(role, Some("  watcher  ")), Some(WATCHER));
    assert!(query::requires_parent(role, Some(PLANNER)));
    assert!(!query::anchors_team(role, Some(PLANNER)));
    assert!(!query::can_spawn(role, Some(WATCHER)));
    assert!(query::anchors_team(role, Some(ORCHESTRATOR)));
    assert!(query::can_spawn(role, Some(GENERAL_AGENT)));
}

#[test]
fn profileless_manager_resolves_to_no_kind_and_is_not_anchoring() {
    let role = Some(AgentRole::Manager);
    for profile in BLANK_PROFILES.iter().copied() {
        assert_eq!(query::kind_id(role, profile), None);
        assert_eq!(query::kind_capability(role, profile), None);
        assert_eq!(query::kind_descriptor(role, profile), None);
        assert!(!query::anchors_team(role, profile));
        assert!(!query::can_spawn(role, profile));
        assert!(query::requires_parent(role, profile));
    }
}

#[test]
fn worker_resolves_to_the_worker_kind_for_any_profile() {
    let role = Some(AgentRole::Worker);
    for profile in ANY_PROFILES.iter().copied() {
        assert_eq!(query::kind_id(role, profile), Some(WORKER));
        assert!(!query::anchors_team(role, profile));
        assert!(!query::can_spawn(role, profile));
        assert!(query::requires_parent(role, profile));
    }
}

#[test]
fn absent_role_resolves_to_no_kind() {
    for profile in ANY_PROFILES.iter().copied() {
        assert_eq!(query::kind_id(None, profile), None);
        assert_eq!(query::kind_capability(None, profile), None);
        assert!(!query::anchors_team(None, profile));
        assert!(!query::can_spawn(None, profile));
    }
}

#[test]
fn unknown_kind_id_resolves_to_leaf_capability() {
    for id in UNKNOWN_KIND_IDS.iter().copied() {
        assert!(!builtins::is_builtin(id), "{id}");
        let capability = builtins::capability_for(id);
        assert_eq!(capability, RoleKindCapability::LEAF, "{id}");
        assert!(!capability.anchors_team(), "{id}");
        assert!(!capability.can_spawn(), "{id}");
        assert!(capability.requires_parent(), "{id}");
    }
}

#[test]
fn unknown_manager_kind_resolves_to_leaf_through_the_query() {
    let role = Some(AgentRole::Manager);
    let leaf = Some(RoleKindCapability::LEAF);
    for id in UNKNOWN_PROFILES.iter().copied() {
        let profile = Some(id);
        assert_eq!(query::kind_capability(role, profile), leaf, "{id}");
        assert_eq!(query::kind_descriptor(role, profile), None, "{id}");
        assert!(!query::anchors_team(role, profile), "{id}");
        assert!(!query::can_spawn(role, profile), "{id}");
        assert!(query::requires_parent(role, profile), "{id}");
    }
}

#[test]
fn capability_fallback_is_leaf_not_all_false() {
    let leaf = RoleKindCapability::LEAF;
    assert_eq!(RoleKindCapability::default(), leaf);
    assert!(leaf.requires_parent());
    assert!(!leaf.anchors_team());
    assert!(!leaf.can_spawn());
}

// `overlay` is the only public constructor and takes no capability argument;
// fields are private and the type is deliberately not `Deserialize`.
#[test]
fn non_builtin_descriptors_are_leaf_by_construction() {
    for id in OVERLAY_IDS.iter().copied() {
        let descriptor = RoleKindDescriptor::overlay(id, "Overlay Kind");
        assert!(!descriptor.anchors_team(), "{id}");
        assert!(!descriptor.can_spawn(), "{id}");
        assert!(descriptor.requires_parent(), "{id}");
        assert_eq!(descriptor.capability(), RoleKindCapability::LEAF);
        assert_eq!(descriptor.origin(), RoleKindOrigin::Overlay, "{id}");
        assert!(!descriptor.is_builtin(), "{id}");
        assert_eq!(descriptor.id(), id);
    }
}

#[test]
fn an_overlay_kind_cannot_shadow_an_embedded_builtin() {
    let forged = RoleKindDescriptor::overlay(ORCHESTRATOR, "Forged");
    let embedded = builtins::lookup(ORCHESTRATOR).expect("builtin");

    assert_ne!(forged.capability(), embedded.capability());
    assert!(!forged.is_builtin());
    assert!(embedded.anchors_team());
    assert!(embedded.can_spawn());
    assert_eq!(embedded.label(), "Orchestrator");
}
