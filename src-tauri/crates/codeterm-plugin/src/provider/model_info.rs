//! The model and reasoning-effort catalog entries every provider publishes.
//! Split out of `provider.rs` to keep that file under the 1200-line limit.

use serde::{Deserialize, Serialize};

/// What a capability probe concluded about one model. `Unknown` is the answer a
/// probe that could not run must give: unavailable means the CLI said no, never
/// that we failed to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailability {
    Available,
    Unavailable,
    Unknown,
}

/// Single model offered by an agent provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Account-can-select-this-now. `None` (common) ⇒ no info, treat as available;
    /// `Some(false)` greys it out in the picker. Set by live discovery; usually `None` in static seeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// The probe's own three-state verdict. `None` ⇒ nothing probed this model,
    /// so `available` alone decides; `Some(Unknown)` ⇒ the probe could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<ModelAvailability>,
    /// `Some(true)` ⇒ the catalog lists this model ahead of the rest, preserving
    /// catalog order within each group. `None`/`Some(false)` ⇒ ordinary order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub featured: Option<bool>,
    /// Short chip rendered beside the label in the spawn model picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// Provider-declared picker section. Models sharing a `group` render together
    /// under that heading; `None` models stay in the flat list. Core never derives
    /// a group from a model id — the provider that owns the id declares it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_efforts: Option<Vec<ReasoningEffortInfo>>,
}

impl ModelInfo {
    /// True when the catalog marks this model to lead the list.
    pub fn is_featured(&self) -> bool {
        self.featured == Some(true)
    }

    /// The one answer every surface renders, so the CLI and the picker cannot
    /// disagree. A model nothing probed and nothing vetoed is available.
    pub fn availability(&self) -> ModelAvailability {
        match (self.availability, self.available) {
            (Some(verdict), _) => verdict,
            (None, Some(false)) => ModelAvailability::Unavailable,
            (None, _) => ModelAvailability::Available,
        }
    }

    /// True when the probe could not reach the thing it was probing.
    pub fn is_unknown(&self) -> bool {
        self.availability() == ModelAvailability::Unknown
    }
}

/// Single reasoning-effort level a provider supports (e.g. codex low/medium/high).
/// Mirrors `ModelInfo`: capability-declared, surfaced in discovery/API/UI. An
/// empty `reasoning_efforts` list means the provider exposes no effort selector.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningEffortInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
