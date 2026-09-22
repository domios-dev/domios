use crate::agent::AgentRole;

use super::builtins::{self, CARTOGRAPHER, GENERAL_AGENT, ORCHESTRATOR, PLANNER, WATCHER, WORKER};
use super::query;
use super::{RoleKindCapability, RoleKindDescriptor, RoleKindOrigin};

struct Row {
    id: &'static str,
    anchors_team: bool,
    requires_parent: bool,
    manages: bool,
    can_spawn: bool,
    team_authority: bool,
    plan_approval: bool,
    plan_edit: bool,
    release: bool,
}

type Caps = (bool, bool, bool, bool, bool, bool, bool, bool);

impl Row {
    fn caps(&self) -> Caps {
        (
            self.anchors_team,
            self.requires_parent,
            self.manages,
            self.can_spawn,
            self.team_authority,
            self.plan_approval,
            self.plan_edit,
            self.release,
        )
    }
}

const ROWS: &[Row] = &[
    Row {
        id: ORCHESTRATOR,
        anchors_team: true,
        requires_parent: false,
        manages: true,
        can_spawn: true,
        team_authority: true,
        plan_approval: true,
        plan_edit: true,
        release: true,
    },
    Row {
        id: GENERAL_AGENT,
        anchors_team: true,
        requires_parent: false,
        manages: true,
        can_spawn: true,
        team_authority: true,
        plan_approval: true,
        plan_edit: true,
        release: true,
    },
    Row {
        id: PLANNER,
        anchors_team: false,
        requires_parent: true,
        manages: true,
        can_spawn: false,
        team_authority: true,
        plan_approval: false,
        plan_edit: true,
        release: false,
    },
    Row {
        id: WATCHER,
        anchors_team: false,
        requires_parent: true,
        manages: true,
        can_spawn: false,
        team_authority: true,
        plan_approval: false,
        plan_edit: false,
        release: false,
    },
    Row {
        id: WORKER,
        anchors_team: false,
        requires_parent: true,
        manages: false,
        can_spawn: false,
        team_authority: false,
        plan_approval: false,
        plan_edit: false,
        release: false,
    },
    Row {
        id: CARTOGRAPHER,
        anchors_team: false,
        requires_parent: true,
        manages: true,
        can_spawn: false,
        team_authority: false,
        plan_approval: false,
        plan_edit: false,
        release: false,
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

fn caps(capability: RoleKindCapability) -> Caps {
    (
        capability.anchors_team(),
        capability.requires_parent(),
        capability.manages(),
        capability.can_spawn(),
        capability.has_team_authority(),
        capability.plan_approval(),
        capability.plan_edit(),
        capability.release(),
    )
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
fn exactly_two_builtin_kinds_anchor_a_team() {
    let anchors: Vec<&str> = builtins::all()
        .iter()
        .filter(|descriptor| descriptor.anchors_team())
        .map(RoleKindDescriptor::id)
        .collect();
    assert_eq!(anchors, vec![ORCHESTRATOR, GENERAL_AGENT]);
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
    assert!(!query::requires_parent(role, Some(ORCHESTRATOR)));
}

#[test]
fn both_anchor_kinds_carry_identical_structural_authority() {
    let role = Some(AgentRole::Manager);
    for anchor in [ORCHESTRATOR, GENERAL_AGENT] {
        let profile = Some(anchor);
        assert!(query::anchors_team(role, profile), "{anchor}");
        assert!(query::can_spawn(role, profile), "{anchor}");
        assert!(!query::requires_parent(role, profile), "{anchor}");
        assert!(query::has_team_authority(role, profile), "{anchor}");
        assert!(query::manages(role, profile), "{anchor}");
        assert!(query::plan_approval(role, profile), "{anchor}");
        assert!(query::plan_edit(role, profile), "{anchor}");
        assert!(query::release(role, profile), "{anchor}");
    }
}

/// Planner and Watcher keep the managing-leaf authority they had: team
/// authority, no anchoring, no spawn. Only Planner drafts plan structure.
#[test]
fn managing_leaf_kinds_hold_team_authority_without_anchoring() {
    let role = Some(AgentRole::Manager);
    for managing in [PLANNER, WATCHER] {
        let profile = Some(managing);
        assert!(query::has_team_authority(role, profile), "{managing}");
        assert!(!query::anchors_team(role, profile), "{managing}");
        assert!(query::requires_parent(role, profile), "{managing}");
        assert!(query::manages(role, profile), "{managing}");
        assert!(!query::can_spawn(role, profile), "{managing}");
        assert!(!query::plan_approval(role, profile), "{managing}");
        assert!(!query::release(role, profile), "{managing}");
    }
    assert!(query::plan_edit(role, Some(PLANNER)));
    assert!(
        !query::plan_edit(role, Some(WATCHER)),
        "the Watcher observes rather than drafts"
    );
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
        assert!(!query::has_team_authority(role, profile));
        assert!(!query::manages(role, profile));
        assert!(!query::plan_approval(role, profile));
        assert!(!query::plan_edit(role, profile));
        assert!(!query::release(role, profile));
        assert!(query::requires_parent(role, profile));
    }
}

#[test]
fn worker_keeps_an_unknown_profile_as_a_leaf_subtype() {
    let role = Some(AgentRole::Worker);
    assert_eq!(
        query::kind_id(role, Some("rust-platform-dev")),
        Some("rust-platform-dev")
    );
    assert!(!query::anchors_team(role, Some("rust-platform-dev")));
    assert!(!query::can_spawn(role, Some("rust-platform-dev")));
    assert!(!query::has_team_authority(role, Some("rust-platform-dev")));
    assert!(!query::manages(role, Some("rust-platform-dev")));
    assert!(query::requires_parent(role, Some("rust-platform-dev")));
}

/// The Cartographer is a Manager-kind managing leaf, not a Worker subtype: a
/// Worker naming it collapses to the plain Worker.
#[test]
fn cartographer_is_a_manager_kind_not_a_worker_leaf_subtype() {
    let profile = Some(CARTOGRAPHER);
    assert_eq!(
        query::kind_id(Some(AgentRole::Worker), profile),
        Some(WORKER)
    );
    assert_eq!(
        query::kind_id(Some(AgentRole::Manager), profile),
        Some(CARTOGRAPHER)
    );
    assert_eq!(
        query::kind_label(Some(AgentRole::Manager), profile),
        Some("Cartographer")
    );
}

#[test]
fn cartographer_is_non_anchoring_and_managing() {
    let role = Some(AgentRole::Manager);
    let profile = Some(CARTOGRAPHER);
    assert!(query::manages(role, profile));
    assert!(query::requires_parent(role, profile));
    assert!(
        !query::anchors_team(role, profile),
        "adds no hierarchy level"
    );
}

/// Every authority the design withholds is false by capability, not by prompt.
#[test]
fn cartographer_withholds_spawn_plan_release_and_team_staffing_authority() {
    let role = Some(AgentRole::Manager);
    let profile = Some(CARTOGRAPHER);
    assert!(!query::can_spawn(role, profile));
    assert!(!query::plan_approval(role, profile));
    assert!(!query::plan_edit(role, profile));
    assert!(!query::release(role, profile));
    assert!(!query::has_team_authority(role, profile));
}

#[test]
fn worker_subtype_cannot_borrow_a_managing_kind() {
    let role = Some(AgentRole::Worker);
    for stolen in [ORCHESTRATOR, GENERAL_AGENT, PLANNER, WATCHER, CARTOGRAPHER] {
        let profile = Some(stolen);
        assert_eq!(query::kind_id(role, profile), Some(WORKER), "{stolen}");
        assert!(!query::anchors_team(role, profile), "{stolen}");
        assert!(!query::can_spawn(role, profile), "{stolen}");
        assert!(!query::has_team_authority(role, profile), "{stolen}");
        assert!(!query::manages(role, profile), "{stolen}");
    }
}

#[test]
fn blank_worker_profile_resolves_to_the_worker_kind() {
    let role = Some(AgentRole::Worker);
    for profile in BLANK_PROFILES.iter().copied() {
        assert_eq!(query::kind_id(role, profile), Some(WORKER));
        assert!(!query::anchors_team(role, profile));
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
        assert!(!query::has_team_authority(None, profile));
        assert!(!query::manages(None, profile));
        assert!(!query::plan_approval(None, profile));
        assert!(!query::plan_edit(None, profile));
        assert!(!query::release(None, profile));
    }
}

#[test]
fn kind_label_prefers_the_builtin_label_and_falls_back_to_the_role() {
    assert_eq!(
        query::kind_label(Some(AgentRole::Manager), Some(ORCHESTRATOR)),
        Some("Orchestrator")
    );
    assert_eq!(
        query::kind_label(Some(AgentRole::Manager), Some(GENERAL_AGENT)),
        Some("General Agent")
    );
    assert_eq!(
        query::kind_label(Some(AgentRole::Manager), Some(PLANNER)),
        Some("Planner")
    );
    assert_eq!(
        query::kind_label(Some(AgentRole::Worker), None),
        Some("Worker")
    );
    assert_eq!(
        query::kind_label(Some(AgentRole::Manager), Some("reviewer")),
        Some("Manager"),
        "an overlay kind falls back to the structural role name"
    );
    assert_eq!(query::kind_label(None, Some(PLANNER)), None);
}

#[test]
fn unknown_kind_id_resolves_to_leaf_capability() {
    for id in UNKNOWN_KIND_IDS.iter().copied() {
        assert!(!builtins::is_builtin(id), "{id}");
        let capability = builtins::capability_for(id);
        assert_eq!(capability, RoleKindCapability::LEAF, "{id}");
        assert!(!capability.anchors_team(), "{id}");
        assert!(!capability.can_spawn(), "{id}");
        assert!(!capability.has_team_authority(), "{id}");
        assert!(!capability.manages(), "{id}");
        assert!(!capability.plan_approval(), "{id}");
        assert!(!capability.plan_edit(), "{id}");
        assert!(!capability.release(), "{id}");
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
        assert!(!query::has_team_authority(role, profile), "{id}");
        assert!(!query::manages(role, profile), "{id}");
        assert!(!query::plan_approval(role, profile), "{id}");
        assert!(!query::plan_edit(role, profile), "{id}");
        assert!(!query::release(role, profile), "{id}");
        assert!(query::requires_parent(role, profile), "{id}");
    }
}

#[test]
fn capability_fallback_is_leaf_not_all_false() {
    let leaf = RoleKindCapability::LEAF;
    assert_eq!(RoleKindCapability::default(), leaf);
    assert!(leaf.requires_parent());
    assert!(!leaf.anchors_team());
    assert!(!leaf.manages());
    assert!(!leaf.can_spawn());
    assert!(!leaf.has_team_authority());
    assert!(!leaf.plan_approval());
    assert!(!leaf.plan_edit());
    assert!(!leaf.release());
}

// `overlay` is the only public constructor and takes no capability argument;
// fields are private and the type is deliberately not `Deserialize`.
#[test]
fn non_builtin_descriptors_are_leaf_by_construction() {
    for id in OVERLAY_IDS.iter().copied() {
        let descriptor = RoleKindDescriptor::overlay(id, "Overlay Kind");
        assert!(!descriptor.anchors_team(), "{id}");
        assert!(!descriptor.manages(), "{id}");
        assert!(!descriptor.can_spawn(), "{id}");
        assert!(!descriptor.has_team_authority(), "{id}");
        assert!(!descriptor.plan_approval(), "{id}");
        assert!(!descriptor.plan_edit(), "{id}");
        assert!(!descriptor.release(), "{id}");
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
