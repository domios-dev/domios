//! One seam for the guidance a role kind carries. Manager kinds and Worker
//! subtypes resolve the same way, keyed by kind id and never a role literal.

use codeterm_types::agent::role_kind::{self, builtins};

use crate::multiplexer::types::AgentRole;

/// Where one guidance fragment came from. Rendered as provenance by the
/// inspection surface and used by occasions to select the fragments they carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FragmentSource {
    /// Bundled text compiled into the binary for a builtin kind.
    Builtin,
    /// The Worker base a leaf kind inherits when it ships no bundled text.
    InheritedBase,
    /// A disk overlay file, named by its path.
    Disk(String),
    /// Caller-supplied persona/guidance owned by this delivery.
    Task,
}

impl FragmentSource {
    /// Contract fragments describe the kind itself; task fragments describe one
    /// delivery. Occasions place the two in different slots.
    pub(crate) fn is_contract(&self) -> bool {
        !matches!(self, Self::Task)
    }

    pub(crate) fn disk_path(&self) -> Option<&str> {
        match self {
            Self::Disk(path) => Some(path),
            _ => None,
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::InheritedBase => "inherited_base",
            Self::Disk(_) => "disk_overlay",
            Self::Task => "task_overlay",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresetFragment {
    pub source: FragmentSource,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RolePreset {
    /// Structural role the kind resolved under. Occasions select rows by this
    /// and by kind id, never by a wire literal.
    pub role: AgentRole,
    pub kind_id: String,
    pub label: String,
    pub fragments: Vec<PresetFragment>,
}

impl RolePreset {
    /// The kind's full guidance: bundled or inherited base, then any disk
    /// overlay, then the task persona. Empty fragments never add a separator.
    pub(crate) fn guidance(&self) -> String {
        self.join(|_| true)
    }

    /// Everything the kind carries by itself, with the delivery's own persona
    /// left out.
    pub(crate) fn contract_guidance(&self) -> String {
        self.join(FragmentSource::is_contract)
    }

    /// Only the persona this delivery supplied.
    pub(crate) fn task_guidance(&self) -> String {
        self.join(|source| !source.is_contract())
    }

    fn join(&self, keep: impl Fn(&FragmentSource) -> bool) -> String {
        self.fragments
            .iter()
            .filter(|fragment| keep(&fragment.source) && !fragment.text.is_empty())
            .map(|fragment| fragment.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Runtime-declared leaf kinds. Implemented by the disk overlay catalog; the
/// resolver never reads a file itself.
pub(crate) trait PresetOverlays {
    fn label_for(&self, kind_id: &str) -> Option<&str>;
    /// Layered guidance plus the path it came from.
    fn guidance_for(&self, kind_id: &str) -> Option<(&str, &str)>;
}

/// The catalog used when no overlay source is installed.
pub(crate) struct NoOverlays;

impl PresetOverlays for NoOverlays {
    fn label_for(&self, _kind_id: &str) -> Option<&str> {
        None
    }

    fn guidance_for(&self, _kind_id: &str) -> Option<(&str, &str)> {
        None
    }
}

/// Bundled guidance for a builtin kind. `general_agent` is absent on purpose:
/// the machine agent composes its own instructions file. Cartographer carries
/// its own Manager contract and never the Worker base.
pub(crate) fn builtin_guidance(kind_id: &str) -> Option<String> {
    match kind_id {
        builtins::ORCHESTRATOR => Some(crate::prompts::orchestrator_suffix()),
        builtins::PLANNER | builtins::WATCHER => Some(crate::prompts::planner_suffix()),
        builtins::WORKER => Some(crate::prompts::worker_suffix()),
        builtins::CARTOGRAPHER => Some(crate::prompts::CARTOGRAPHER_SUFFIX.to_string()),
        _ => None,
    }
}

/// An overlay kind has no bundled text, so it inherits its structural base and
/// layers the supplied persona on top — the same composition managers get.
#[allow(dead_code)]
pub(crate) fn resolve(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    overlay_guidance: Option<&str>,
) -> Option<RolePreset> {
    resolve_in(&NoOverlays, role, role_profile, overlay_guidance)
}

pub(crate) fn resolve_in(
    overlays: &dyn PresetOverlays,
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    overlay_guidance: Option<&str>,
) -> Option<RolePreset> {
    let resolved_role = role?;
    let kind_id = role_kind::kind_id(role, role_profile)?.to_string();
    // Builtin rows are embedded and non-overridable, so a catalog entry that
    // shares their id contributes nothing even if a loader ever admitted it.
    let overlays: &dyn PresetOverlays = if builtins::is_builtin(&kind_id) {
        &NoOverlays
    } else {
        overlays
    };
    let label = overlays
        .label_for(&kind_id)
        .map(str::to_string)
        .or_else(|| role_kind::kind_label(role, role_profile).map(str::to_string))
        .unwrap_or_else(|| kind_id.clone());
    let kind_manages = builtins::lookup(&kind_id).is_some_and(|descriptor| descriptor.manages());
    let mut fragments = Vec::new();
    match builtin_guidance(&kind_id) {
        Some(guidance) => fragments.push(PresetFragment {
            source: FragmentSource::Builtin,
            text: guidance,
        }),
        // Only a leaf overlay inherits a base; a Manager-kind role carries its
        // own contract and an unknown manager kind must state its own.
        None if role == Some(AgentRole::Worker) && !kind_manages => {
            fragments.push(PresetFragment {
                source: FragmentSource::InheritedBase,
                text: crate::prompts::worker_suffix(),
            })
        }
        None => {}
    }
    if let Some((guidance, path)) = overlays.guidance_for(&kind_id) {
        let guidance = guidance.trim();
        if !guidance.is_empty() {
            fragments.push(PresetFragment {
                source: FragmentSource::Disk(path.to_string()),
                text: guidance.to_string(),
            });
        }
    }
    if let Some(overlay) = overlay_guidance
        .map(str::trim)
        .filter(|guidance| !guidance.is_empty())
    {
        fragments.push(PresetFragment {
            source: FragmentSource::Task,
            text: overlay.to_string(),
        });
    }
    Some(RolePreset {
        role: resolved_role,
        kind_id,
        label,
        fragments,
    })
}

#[cfg(test)]
#[path = "role_preset/tests.rs"]
mod tests;
