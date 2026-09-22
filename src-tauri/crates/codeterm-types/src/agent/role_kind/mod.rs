use std::borrow::Cow;

use serde::Serialize;

pub mod builtins;
pub mod query;

#[cfg(test)]
mod tests;

pub use builtins::all as builtin_kinds;
pub use builtins::capability_for as capability_for_kind;
pub use builtins::lookup as builtin_kind;
pub use query::{anchors_team, can_spawn, has_team_authority, manages, plan_approval, plan_edit};
pub use query::{kind_capability, kind_descriptor, kind_id, kind_label};
pub use query::{release, requires_parent};

/// Structural authority of a role kind. Fields are private and `LEAF` is the
/// only public value, so capability cannot be named or forged off-crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RoleKindCapability {
    anchors_team: bool,
    requires_parent: bool,
    manages: bool,
    can_spawn: bool,
    team_authority: bool,
    plan_approval: bool,
    plan_edit: bool,
    release: bool,
}

impl RoleKindCapability {
    pub const LEAF: Self = Self {
        anchors_team: false,
        requires_parent: true,
        manages: false,
        can_spawn: false,
        team_authority: false,
        plan_approval: false,
        plan_edit: false,
        release: false,
    };

    const ANCHORING: Self = Self {
        anchors_team: true,
        requires_parent: false,
        manages: true,
        can_spawn: true,
        team_authority: true,
        plan_approval: true,
        plan_edit: true,
        release: true,
    };

    /// A manager that lives under an anchor: no Team of its own, but it may
    /// rename, adopt into, and close the Team it belongs to.
    const MANAGING_LEAF: Self = Self {
        anchors_team: false,
        requires_parent: true,
        manages: true,
        can_spawn: false,
        team_authority: true,
        plan_approval: false,
        plan_edit: true,
        release: false,
    };

    /// A managing leaf that observes rather than drafts: team authority without
    /// plan structure mutation.
    const MANAGING_LEAF_OBSERVER: Self = Self {
        anchors_team: false,
        requires_parent: true,
        manages: true,
        can_spawn: false,
        team_authority: true,
        plan_approval: false,
        plan_edit: false,
        release: false,
    };

    /// A managing leaf that authors one artifact and nothing else: no spawn, no
    /// plan approval or editing, no release, and no team staffing.
    const MANAGING_LEAF_WITHHELD: Self = Self {
        anchors_team: false,
        requires_parent: true,
        manages: true,
        can_spawn: false,
        team_authority: false,
        plan_approval: false,
        plan_edit: false,
        release: false,
    };

    pub const fn anchors_team(self) -> bool {
        self.anchors_team
    }

    pub const fn requires_parent(self) -> bool {
        self.requires_parent
    }

    /// True for a Manager-kind role rather than a leaf.
    pub const fn manages(self) -> bool {
        self.manages
    }

    pub const fn can_spawn(self) -> bool {
        self.can_spawn
    }

    /// Team staffing and Team lifecycle: rename, adopt into, and close.
    pub const fn has_team_authority(self) -> bool {
        self.team_authority
    }

    pub const fn plan_approval(self) -> bool {
        self.plan_approval
    }

    pub const fn plan_edit(self) -> bool {
        self.plan_edit
    }

    pub const fn release(self) -> bool {
        self.release
    }
}

// A derived Default would clear `requires_parent`; every fallback lands on leaf.
impl Default for RoleKindCapability {
    fn default() -> Self {
        Self::LEAF
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleKindOrigin {
    Builtin,
    Overlay,
}

/// Structural declaration of one role kind. Fields are private and the type is
/// deliberately not `Deserialize` — a decoded descriptor would forge capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleKindDescriptor {
    id: Cow<'static, str>,
    label: Cow<'static, str>,
    capability: RoleKindCapability,
    origin: RoleKindOrigin,
}

impl RoleKindDescriptor {
    const fn new(id: &'static str, label: &'static str, capability: RoleKindCapability) -> Self {
        Self {
            id: Cow::Borrowed(id),
            label: Cow::Borrowed(label),
            capability,
            origin: RoleKindOrigin::Builtin,
        }
    }

    const fn anchoring(id: &'static str, label: &'static str) -> Self {
        Self::new(id, label, RoleKindCapability::ANCHORING)
    }

    const fn managing_leaf(id: &'static str, label: &'static str) -> Self {
        Self::new(id, label, RoleKindCapability::MANAGING_LEAF)
    }

    const fn managing_leaf_observer(id: &'static str, label: &'static str) -> Self {
        Self::new(id, label, RoleKindCapability::MANAGING_LEAF_OBSERVER)
    }

    const fn managing_leaf_withheld(id: &'static str, label: &'static str) -> Self {
        Self::new(id, label, RoleKindCapability::MANAGING_LEAF_WITHHELD)
    }

    const fn leaf(id: &'static str, label: &'static str) -> Self {
        Self::new(id, label, RoleKindCapability::LEAF)
    }

    /// The only public constructor. Capability is hard-coded leaf and is not a
    /// parameter, so a runtime-defined kind can never anchor a Team or spawn.
    pub fn overlay(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: Cow::Owned(id.into()),
            label: Cow::Owned(label.into()),
            capability: RoleKindCapability::LEAF,
            origin: RoleKindOrigin::Overlay,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn capability(&self) -> RoleKindCapability {
        self.capability
    }

    pub fn origin(&self) -> RoleKindOrigin {
        self.origin
    }

    pub fn is_builtin(&self) -> bool {
        matches!(self.origin, RoleKindOrigin::Builtin)
    }

    pub fn anchors_team(&self) -> bool {
        self.capability.anchors_team()
    }

    pub fn requires_parent(&self) -> bool {
        self.capability.requires_parent()
    }

    pub fn manages(&self) -> bool {
        self.capability.manages()
    }

    pub fn can_spawn(&self) -> bool {
        self.capability.can_spawn()
    }

    pub fn has_team_authority(&self) -> bool {
        self.capability.has_team_authority()
    }

    pub fn plan_approval(&self) -> bool {
        self.capability.plan_approval()
    }

    pub fn plan_edit(&self) -> bool {
        self.capability.plan_edit()
    }

    pub fn release(&self) -> bool {
        self.capability.release()
    }
}
