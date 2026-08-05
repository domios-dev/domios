use std::borrow::Cow;

use serde::Serialize;

pub mod builtins;
pub mod query;

#[cfg(test)]
mod tests;

pub use builtins::all as builtin_kinds;
pub use builtins::capability_for as capability_for_kind;
pub use builtins::lookup as builtin_kind;
pub use query::{anchors_team, can_spawn, has_team_authority, requires_parent};
pub use query::{kind_capability, kind_descriptor, kind_id, kind_label};

/// Structural authority of a role kind. Fields are private and `LEAF` is the
/// only public value, so capability cannot be named or forged off-crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RoleKindCapability {
    anchors_team: bool,
    requires_parent: bool,
    can_spawn: bool,
    team_authority: bool,
}

impl RoleKindCapability {
    pub const LEAF: Self = Self {
        anchors_team: false,
        requires_parent: true,
        can_spawn: false,
        team_authority: false,
    };

    const ANCHORING: Self = Self {
        anchors_team: true,
        requires_parent: false,
        can_spawn: true,
        team_authority: true,
    };

    /// A manager that lives under an anchor: no Team of its own, but it may
    /// rename, adopt into, and close the Team it belongs to.
    const MANAGING_LEAF: Self = Self {
        anchors_team: false,
        requires_parent: true,
        can_spawn: false,
        team_authority: true,
    };

    pub const fn anchors_team(self) -> bool {
        self.anchors_team
    }

    pub const fn requires_parent(self) -> bool {
        self.requires_parent
    }

    pub const fn can_spawn(self) -> bool {
        self.can_spawn
    }

    pub const fn has_team_authority(self) -> bool {
        self.team_authority
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

    pub fn can_spawn(&self) -> bool {
        self.capability.can_spawn()
    }

    pub fn has_team_authority(&self) -> bool {
        self.capability.has_team_authority()
    }
}
