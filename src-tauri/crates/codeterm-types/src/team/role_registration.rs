use codeterm_team_package::{ManagerSlot, ModelPreference, RoleStart, TeamRoleGroup};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The package a registration is pinned to; a role never moves to another version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct PinnedPackage {
    pub name: String,
    pub version: String,
}

/// A role file the registration hands its role, with the description it carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoleFileRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

/// One role type registered for a Team. Immutable once stored; it never spawns a
/// Worker and never widens the structural authority of its manager slot or worker name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoleRegistration {
    pub team_id: String,
    pub group: TeamRoleGroup,
    /// The manager slot id or the worker name.
    pub name: String,
    /// Shown as the tab's role label: the manager display name or the worker name.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub start: RoleStart,
    pub package: PinnedPackage,
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<ModelPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reasoning: Option<String>,
    /// The mesh peer that hosts this role's Workers when dispatch places it remotely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub placement_peer: Option<String>,
    /// The preview consent decision this registration was accepted under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub consent_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<RoleFileRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    pub registered_at: u64,
}

impl RoleRegistration {
    pub fn slot(&self) -> Option<ManagerSlot> {
        match self.group {
            TeamRoleGroup::Manager => ManagerSlot::parse(&self.name),
            TeamRoleGroup::Worker => None,
        }
    }

    /// Two registrations of one role are the same when everything but the clock agrees.
    pub fn same_content(&self, other: &Self) -> bool {
        Self {
            registered_at: 0,
            ..self.clone()
        } == Self {
            registered_at: 0,
            ..other.clone()
        }
    }
}

/// How a registration request settled against what the Team already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RoleRegistrationOutcome {
    Created,
    AlreadyPresent,
    /// The role is pinned to another package version or content and stays as it was.
    Pinned,
}

/// Where a resolved role's text came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum RoleProvenance {
    Builtin { package: PinnedPackage },
    Team { package: PinnedPackage },
}
