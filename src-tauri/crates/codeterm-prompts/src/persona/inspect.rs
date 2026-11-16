//! Read-only view of the shared resolver: exactly the text a delivery occasion
//! would emit, with the provenance of every fragment.

use codeterm_team_package::{RoleKind, RoleStart, StartMode};
use codeterm_types::agent::role_kind::{self, builtins, RoleType};
use codeterm_types::agent::AgentRole;
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
    pub kind: RoleKind,
    /// The Domios authority the name carries: a built-in manager's label, or `none`.
    pub authority: &'static str,
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
    pub kind: RoleKind,
    /// The Domios authority the name carries: a built-in manager's label, or `none`.
    pub authority: &'static str,
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
        kind: preset.role.package_kind(),
        authority: role_preset::authority_label(preset.role, &role_type.kind_id),
        label: preset.label.clone(),
        provenance: preset.provenance().cloned(),
        guidance: preset.guidance(),
        fragments: fragment_views(&preset),
        occasions,
    })
}

fn listing(
    role_type: &str,
    kind: RoleKind,
    description: Option<String>,
    start: RoleStart,
    provenance: RoleProvenance,
    label: String,
) -> RoleListing {
    let role = AgentRole::from(kind);
    let capability = role_kind::registered_capability(role, role_type);
    RoleListing {
        role_type: role_type.to_string(),
        kind,
        authority: role_preset::authority_label(role, role_type),
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

/// Every role the Team has: exactly its registrations (the default package as a whole when it
/// has none), then the host-only Watcher.
pub fn list(team_id: Option<&str>, team: &[RoleRegistration]) -> RegistryView {
    let (team, builtin) = role_preset::effective(team);
    let mut roles: Vec<RoleListing> = team
        .iter()
        .map(|registration| {
            let package = registration.package.clone();
            listing(
                &registration.name,
                registration.kind,
                registration.description.clone(),
                registration.start.clone(),
                if builtin {
                    RoleProvenance::Builtin { package }
                } else {
                    RoleProvenance::Team { package }
                },
                registration.label.clone(),
            )
        })
        .collect();
    roles.push(listing(
        builtins::WATCHER,
        RoleKind::Manager,
        None,
        RoleStart::Mode(StartMode::OnDemand),
        RoleProvenance::Builtin {
            package: builtin_roles::default_identity(),
        },
        role_kind::builtin_kind(builtins::WATCHER)
            .map_or(String::new(), |kind| kind.label().to_string()),
    ));
    RegistryView {
        team_id: team_id.map(str::to_string),
        roles,
    }
}
