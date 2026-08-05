//! Read-only view of the shared resolver: exactly the text a delivery occasion
//! would emit, with the provenance of every fragment.

use codeterm_types::agent::role_kind::builtins;
use serde::Serialize;

use crate::multiplexer::types::AgentRole;
use crate::service::role_preset::{self, RolePreset};

use super::occasion::{self, Occasion};
use super::overlay::{self, OverlayCatalog};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct FragmentView {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct OccasionView {
    pub occasion: &'static str,
    /// Where this occasion's text came from: the preset or its declared fallback.
    pub emission: &'static str,
    /// The exact text the occasion contributes, before any payload is attached.
    pub text: String,
    /// The full delivery for a probe payload, so ordering and separators show.
    pub composed: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PresetView {
    pub role: &'static str,
    pub kind_id: String,
    pub label: String,
    pub origin: &'static str,
    /// Every fragment joined — the kind's full guidance for this delivery.
    pub guidance: String,
    pub fragments: Vec<FragmentView>,
    pub occasions: Vec<OccasionView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct KindListing {
    pub kind_id: String,
    pub label: String,
    pub origin: &'static str,
    pub anchors_team: bool,
    pub team_authority: bool,
    pub can_spawn: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RejectionView {
    pub path: String,
    pub reason: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CatalogView {
    pub overlay_dir: String,
    pub kinds: Vec<KindListing>,
    pub rejected: Vec<RejectionView>,
}

/// Payload used so the rendered composition shows separators and ordering.
pub(crate) const PROBE_PAYLOAD: &str = "<assignment>";

fn origin_label(catalog: &OverlayCatalog, kind_id: &str) -> &'static str {
    if builtins::is_builtin(kind_id) {
        return "builtin";
    }
    if catalog.kinds().any(|kind| kind.id == kind_id) {
        return "overlay";
    }
    "unregistered"
}

fn fragment_views(preset: &RolePreset) -> Vec<FragmentView> {
    preset
        .fragments
        .iter()
        .map(|fragment| FragmentView {
            source: fragment.source.label(),
            path: fragment.source.disk_path().map(str::to_string),
            text: fragment.text.clone(),
        })
        .collect()
}

fn occasion_views(preset: &RolePreset) -> Vec<OccasionView> {
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

/// Render one `(role, kind, task persona)` triple through the shared resolver.
/// `only` narrows the rendering to a single occasion.
pub(crate) fn show(
    catalog: &OverlayCatalog,
    role: AgentRole,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
    only: Option<Occasion>,
) -> Option<PresetView> {
    let preset = role_preset::resolve_in(catalog, Some(role), role_profile, task_guidance)?;
    let occasions = occasion_views(&preset)
        .into_iter()
        .filter(|view| only.map_or(true, |wanted| wanted.id() == view.occasion))
        .collect();
    Some(PresetView {
        role: role.label(),
        origin: origin_label(catalog, &preset.kind_id),
        kind_id: preset.kind_id.clone(),
        label: preset.label.clone(),
        guidance: preset.guidance(),
        fragments: fragment_views(&preset),
        occasions,
    })
}

/// Every kind the resolver can reach, builtin and overlay, plus the overlay
/// files that were refused and why.
pub(crate) fn list(catalog: &OverlayCatalog) -> CatalogView {
    let mut kinds: Vec<KindListing> = builtins::all()
        .iter()
        .map(|descriptor| KindListing {
            kind_id: descriptor.id().to_string(),
            label: descriptor.label().to_string(),
            origin: "builtin",
            anchors_team: descriptor.anchors_team(),
            team_authority: descriptor.has_team_authority(),
            can_spawn: descriptor.can_spawn(),
            path: None,
        })
        .collect();
    kinds.extend(catalog.kinds().map(|kind| KindListing {
        kind_id: kind.id.clone(),
        label: kind.label.clone(),
        origin: "overlay",
        anchors_team: false,
        team_authority: false,
        can_spawn: false,
        path: Some(kind.path.clone()),
    }));
    CatalogView {
        overlay_dir: overlay::overlay_dir().to_string_lossy().replace('\\', "/"),
        kinds,
        rejected: catalog
            .rejected()
            .iter()
            .map(|rejection| RejectionView {
                path: rejection.path.clone(),
                reason: rejection.reason.code(),
                detail: rejection.reason.detail(),
            })
            .collect(),
    }
}
