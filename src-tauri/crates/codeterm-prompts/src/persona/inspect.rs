//! Read-only view of the shared resolver: exactly the text a delivery occasion
//! would emit, with the provenance of every fragment.

use codeterm_team_package::{ManagerSlot, RoleStart, TeamRoleGroup};
use codeterm_types::agent::role_kind::{self, builtins, RoleType};
use codeterm_types::team::role_registration::{PinnedPackage, RoleProvenance, RoleRegistration};
use serde::Serialize;

use crate::builtin_roles;
use crate::role_preset::{self, RolePreset};

use super::occasion::{self, Occasion};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FragmentView {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<PinnedPackage>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OccasionView {
    pub occasion: &'static str,
    /// Where this occasion's text came from: the preset or its declared fallback.
    pub emission: &'static str,
    /// The exact text the occasion contributes, before any payload is attached.
    pub text: String,
    /// The full delivery for a probe payload, so ordering and separators show.
    pub composed: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PresetView {
    #[serde(rename = "type")]
    pub role_type: String,
    pub role: &'static str,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<RoleProvenance>,
    /// Every fragment joined — the role's full guidance for this delivery.
    pub guidance: String,
    pub fragments: Vec<FragmentView>,
    pub occasions: Vec<OccasionView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RoleListing {
    #[serde(rename = "type")]
    pub role_type: String,
    pub group: TeamRoleGroup,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: RoleStart,
    pub provenance: RoleProvenance,
    pub host_only: bool,
    pub anchors_team: bool,
    pub team_authority: bool,
    pub can_spawn: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegistryView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub roles: Vec<RoleListing>,
}

/// Payload used so the rendered composition shows separators and ordering.
pub const PROBE_PAYLOAD: &str = "<assignment>";

pub fn fragment_views(preset: &RolePreset) -> Vec<FragmentView> {
    preset
        .fragments
        .iter()
        .map(|fragment| FragmentView {
            source: fragment.source.label(),
            package: fragment.source.package().cloned(),
            text: fragment.text.clone(),
        })
        .collect()
}

pub fn occasion_views(preset: &RolePreset) -> Vec<OccasionView> {
    occasion::ALL
        .iter()
        .map(|&which| {
            let (text, emission) = occasion::guidance(which, Some(preset));
            OccasionView {
                occasion: which.id(),
                emission: emission.label(),
                text,
                composed: occasion::compose(which, Some(preset), Some(PROBE_PAYLOAD)),
            }
        })
        .collect()
}

/// Render one role type through the shared resolver. `only` narrows the rendering to one occasion.
pub fn show(
    team: &[RoleRegistration],
    role_type: &RoleType,
    task_guidance: Option<&str>,
    only: Option<Occasion>,
) -> Option<PresetView> {
    let preset = role_preset::resolve_in(
        team,
        Some(role_type.role),
        Some(&role_type.kind_id),
        task_guidance,
    )?;
    let occasions = occasion_views(&preset)
        .into_iter()
        .filter(|view| only.map_or(true, |wanted| wanted.id() == view.occasion))
        .collect();
    Some(PresetView {
        role_type: role_type.kind_id.clone(),
        role: preset.role.label(),
        label: preset.label.clone(),
        provenance: preset.provenance().cloned(),
        guidance: preset.guidance(),
        fragments: fragment_views(&preset),
        occasions,
    })
}

fn listing(
    role_type: &str,
    group: TeamRoleGroup,
    description: Option<String>,
    start: RoleStart,
    provenance: RoleProvenance,
    label: String,
) -> RoleListing {
    let capability = role_kind::capability_for_kind(role_type);
    RoleListing {
        role_type: role_type.to_string(),
        group,
        label,
        description,
        start,
        provenance,
        host_only: role_type == builtins::WATCHER,
        anchors_team: capability.anchors_team(),
        team_authority: capability.has_team_authority(),
        can_spawn: capability.can_spawn(),
    }
}

/// Every role type the Team can launch: its registrations, then the default package for the rest.
pub fn list(team_id: Option<&str>, team: &[RoleRegistration]) -> RegistryView {
    let package = builtin_roles::default_package();
    let builtin = || RoleProvenance::Builtin {
        package: builtin_roles::default_identity(),
    };
    let registered = |group: TeamRoleGroup, name: &str| {
        team.iter()
            .find(|registration| registration.group == group && registration.name == name)
    };
    let mut roles = Vec::new();
    for slot in ManagerSlot::ALL {
        let name = slot.as_str();
        roles.push(match registered(TeamRoleGroup::Manager, name) {
            Some(registration) => entry_for(registration),
            None => {
                let role = package.document.managers.slot(slot);
                listing(
                    name,
                    TeamRoleGroup::Manager,
                    role.and_then(|role| role.description.clone()),
                    role.map_or_else(
                        || slot.default_start(),
                        |role| role.effective_start(Some(slot)),
                    ),
                    builtin(),
                    role_kind::builtin_kind(name)
                        .map_or(name.to_string(), |kind| kind.label().to_string()),
                )
            }
        });
    }
    roles.push(listing(
        builtins::WATCHER,
        TeamRoleGroup::Manager,
        None,
        RoleStart::Mode(codeterm_team_package::StartMode::OnDemand),
        builtin(),
        role_kind::builtin_kind(builtins::WATCHER)
            .map_or(String::new(), |kind| kind.label().to_string()),
    ));
    for (name, role) in package.document.workers.iter() {
        if registered(TeamRoleGroup::Worker, name).is_none() {
            roles.push(listing(
                name,
                TeamRoleGroup::Worker,
                role.description.clone(),
                role.effective_start(None),
                builtin(),
                name.to_string(),
            ));
        }
    }
    roles.extend(
        team.iter()
            .filter(|registration| registration.group == TeamRoleGroup::Worker)
            .map(entry_for),
    );
    RegistryView {
        team_id: team_id.map(str::to_string),
        roles,
    }
}

fn entry_for(registration: &RoleRegistration) -> RoleListing {
    listing(
        &registration.name,
        registration.group,
        registration.description.clone(),
        registration.start.clone(),
        RoleProvenance::Team {
            package: registration.package.clone(),
        },
        registration.label.clone(),
    )
}
