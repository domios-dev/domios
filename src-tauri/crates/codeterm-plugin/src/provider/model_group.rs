//! Provider-declared model grouping and per-group usage.
//!
//! A provider owns how its models are grouped (for example by the upstream
//! company a routed model belongs to) and where each group's usage comes from.
//! Core only carries the declaration to the picker; it never maps a vendor id
//! to a label or knows a provider's storage layout.

use serde::{Deserialize, Serialize};

/// One provider-owned grouping of its models. `id` is the model-id prefix the
/// provider publishes (`deepseek` for `deepseek/...`); `label` is the text the
/// picker renders for the section. Both are the provider's to decide.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGroup {
    pub id: String,
    pub label: String,
}

/// Cumulative usage the provider attributes to one model group. Tokens are raw
/// counts; `cost_micros` is USD × 1_000_000 so the wire stays integer. A group
/// the provider reports with no observed turns carries zeroes, never `None`:
/// "connected but unused" and "no such group" are different facts.
///
/// The optional `spend_*` fields are the group's spend meter for a token-billed
/// upstream: `spend_cents` is what the provider observed, the limit/remaining
/// pair is only present when the provider can read a live quota. A group whose
/// live limit is unreadable leaves the limit fields `None` rather than guessing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGroupUsage {
    /// Matches `ModelGroup::id`.
    pub group: String,
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub cost_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_cents: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_limit_cents: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_remaining_cents: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_resets_at_ms: Option<i64>,
}
