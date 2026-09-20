//! One declarative table maps a delivery occasion and a resolved `RolePreset`
//! to the text that occasion emits. Every occasion declares a fallback.

use codeterm_types::agent::role_kind::builtins;

use crate::multiplexer::types::AgentRole;
use crate::service::role_preset::RolePreset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Occasion {
    /// Role guidance decorating a launch assignment.
    LaunchAssignment,
    /// The identity slot leading a Worker's launch system prompt.
    WorkerLaunchIdentity,
    /// The `{role_guidance}` slot of the Team manager bootstrap.
    ManagerBootstrap,
}

impl Occasion {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::LaunchAssignment => "launch_assignment",
            Self::WorkerLaunchIdentity => "worker_launch_identity",
            Self::ManagerBootstrap => "manager_bootstrap",
        }
    }

    pub(crate) fn parse(id: &str) -> Option<Self> {
        ALL.iter().copied().find(|occasion| occasion.id() == id)
    }
}

pub(crate) const ALL: &[Occasion] = &[
    Occasion::LaunchAssignment,
    Occasion::WorkerLaunchIdentity,
    Occasion::ManagerBootstrap,
];

/// Which resolved presets a row answers for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selector {
    /// One builtin kind id. A Worker subtype can never resolve to a managing
    /// kind id, so these rows only ever match Manager presets.
    Kind(&'static str),
    /// The builtin Worker and every leaf subtype resolved under it.
    WorkerLeaf,
    /// Any kind resolved under the Manager role.
    ManagerKind,
}

impl Selector {
    fn matches(self, preset: &RolePreset) -> bool {
        match self {
            Self::Kind(kind_id) => preset.kind_id == kind_id,
            Self::WorkerLeaf => preset.role == AgentRole::Worker,
            Self::ManagerKind => preset.role == AgentRole::Manager,
        }
    }
}

/// Which fragments of the preset an occasion consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// The kind's own contract: bundled or inherited base plus disk overlays.
    Contract,
    /// Only the persona this delivery supplied.
    TaskPersona,
    /// This pair contributes no preset text.
    Silent,
}

/// How an occasion places preset text next to its own payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placement {
    slot: Slot,
    /// Inserted between the payload and the preset text.
    separator: &'static str,
    /// Emit preset text even when the occasion carries no payload.
    standalone: bool,
}

impl Placement {
    const SILENT: Self = Self {
        slot: Slot::Silent,
        separator: "",
        standalone: false,
    };
}

/// Applied when no row matches, or when a matching row's slot is empty. Not
/// comparable: equality on the declared `fn` pointer would be unpredictable.
#[derive(Debug, Clone, Copy)]
enum Fallback {
    /// Emit the payload alone. Non-empty whenever the occasion has a payload,
    /// which is the only case this occasion emits at all.
    PayloadOnly,
    /// Declared text used in place of absent preset guidance.
    Declared(fn() -> String),
}

struct OccasionRules {
    occasion: Occasion,
    rows: &'static [(Selector, Placement)],
    fallback: Fallback,
}

/// The default identity of a Worker launched without a task-owned persona.
fn default_worker_identity() -> String {
    "You are a CodeTerm Worker.".to_string()
}

static TABLE: &[OccasionRules] = &[
    OccasionRules {
        occasion: Occasion::LaunchAssignment,
        rows: &[
            (
                Selector::Kind(builtins::ORCHESTRATOR),
                Placement {
                    slot: Slot::Contract,
                    separator: "\n\n",
                    standalone: true,
                },
            ),
            (
                Selector::Kind(builtins::PLANNER),
                Placement {
                    slot: Slot::Contract,
                    separator: "",
                    standalone: false,
                },
            ),
            (
                Selector::Kind(builtins::CARTOGRAPHER),
                Placement {
                    slot: Slot::Contract,
                    separator: "",
                    standalone: false,
                },
            ),
            (Selector::Kind(builtins::WATCHER), Placement::SILENT),
            (
                Selector::Kind(builtins::CARTOGRAPHER),
                Placement {
                    slot: Slot::Contract,
                    separator: "\n\n",
                    standalone: true,
                },
            ),
            (Selector::Kind(builtins::GENERAL_AGENT), Placement::SILENT),
            (
                Selector::WorkerLeaf,
                Placement {
                    slot: Slot::Contract,
                    separator: "",
                    standalone: false,
                },
            ),
        ],
        fallback: Fallback::PayloadOnly,
    },
    OccasionRules {
        occasion: Occasion::WorkerLaunchIdentity,
        rows: &[(
            Selector::WorkerLeaf,
            Placement {
                slot: Slot::TaskPersona,
                separator: "",
                standalone: true,
            },
        )],
        fallback: Fallback::Declared(default_worker_identity),
    },
    OccasionRules {
        occasion: Occasion::ManagerBootstrap,
        rows: &[(
            Selector::ManagerKind,
            Placement {
                slot: Slot::Contract,
                separator: "",
                standalone: true,
            },
        )],
        fallback: Fallback::Declared(crate::prompts::planner_suffix),
    },
];

fn rules(occasion: Occasion) -> &'static OccasionRules {
    TABLE
        .iter()
        .find(|rules| rules.occasion == occasion)
        .expect("every occasion declares a row set")
}

fn placement(occasion: Occasion, preset: Option<&RolePreset>) -> Option<Placement> {
    let preset = preset?;
    rules(occasion)
        .rows
        .iter()
        .find(|(selector, _)| selector.matches(preset))
        .map(|(_, placement)| *placement)
}

fn slot_text(slot: Slot, preset: &RolePreset) -> String {
    match slot {
        Slot::Contract => preset.contract_guidance(),
        Slot::TaskPersona => preset.task_guidance(),
        Slot::Silent => String::new(),
    }
}

/// Where an occasion's text came from — the resolved preset, the occasion's
/// declared fallback, or nowhere because the pair contributes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emission {
    Preset,
    Fallback,
    Silent,
}

impl Emission {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Preset => "preset",
            Self::Fallback => "declared_fallback",
            Self::Silent => "silent",
        }
    }
}

/// The text declared for unmatched pairs. `None` means the occasion emits only
/// its own payload, which is itself the declared fallback.
pub(crate) fn declared_fallback(occasion: Occasion) -> Option<String> {
    match rules(occasion).fallback {
        Fallback::PayloadOnly => None,
        Fallback::Declared(text) => Some(text()),
    }
}

/// Text this occasion contributes, before it is placed next to a payload.
pub(crate) fn guidance(occasion: Occasion, preset: Option<&RolePreset>) -> (String, Emission) {
    let placement = placement(occasion, preset);
    if matches!(
        placement.map(|placement| placement.slot),
        Some(Slot::Silent)
    ) {
        return (String::new(), Emission::Silent);
    }
    let from_preset = placement
        .zip(preset)
        .map(|(placement, preset)| slot_text(placement.slot, preset))
        .unwrap_or_default();
    if !from_preset.is_empty() {
        return (from_preset, Emission::Preset);
    }
    match declared_fallback(occasion) {
        Some(text) => (text, Emission::Fallback),
        None => (String::new(), Emission::Silent),
    }
}

/// Compose one delivery: the occasion's payload plus the guidance the table
/// assigns to this preset. `None` means the occasion emits nothing at all.
pub(crate) fn compose(
    occasion: Occasion,
    preset: Option<&RolePreset>,
    payload: Option<&str>,
) -> Option<String> {
    let (text, _) = guidance(occasion, preset);
    let placement = placement(occasion, preset);
    let separator = placement.map(|placement| placement.separator).unwrap_or("");
    let standalone = placement
        .map(|placement| placement.standalone)
        .unwrap_or(false);
    match (payload, text.is_empty()) {
        (Some(payload), false) => Some(format!("{payload}{separator}{text}")),
        (Some(payload), true) => Some(payload.to_string()),
        (None, false) if standalone => Some(text),
        (None, _) => None,
    }
}
