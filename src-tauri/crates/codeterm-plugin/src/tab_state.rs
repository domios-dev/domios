//! Generic, provider-agnostic tab-state classification types (AS1 seam). Core
//! asks a provider to classify a snapshot and never branches on a provider id or
//! screen string; all provider-specific detection lives in the plugin, which
//! returns one of these generic verdicts for the reactor to execute.

use serde::{Deserialize, Serialize};

/// The classified lifecycle state of a parked tab. Generic across providers —
/// a Codex update prompt and a Claude rate-limit menu both map onto this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabState {
    /// Still actively working — no intervention warranted.
    Working,
    /// Login / OAuth / API-key prompt. SACRED: never auto-advanced.
    WaitingOnAuth,
    /// A CLI self-update / version prompt parked on a default we may not want.
    UpdatePrompt,
    /// Provider hit a usage/rate limit; may carry a reset time for a resume.
    RateLimited,
    /// Agent is asking the user a question that needs surfacing (not answering).
    ClarifyingQuestion,
    /// A turn is owed but the tab is parked — "should be working but isn't".
    IdleStuck,
    /// Classifier could not confidently determine the state.
    Unknown,
}

/// The tab snapshot handed to a provider classifier. Built by core from the
/// tab's OWN session/screen — the provider gets only what it needs and never
/// reaches back into core state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStateSnapshot {
    pub tab_id: String,
    pub agent_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Recent rendered screen tail (provider fingerprints match against this).
    pub screen_text: String,
}

/// Optional structured payload extracted by the classifier.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStateData {
    /// ISO-8601 reset time for a rate limit, if parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<String>,
    /// Clarifying questions to surface to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<String>>,
}

/// A delayed resume to schedule via the T7 delayed-send machinery: send `text`
/// at `at`, then optionally press `then_keys`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayedResume {
    /// ISO-8601 wall-clock time to fire the resume.
    pub at: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then_keys: Option<Vec<String>>,
}

/// A user-facing surface: a chat card + a notification. `kind` names the card
/// template; `payload` is the generic body the frontend renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Surface {
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpdatePromptAction {
    pub install_keys: Vec<String>,
    pub decline_keys: Vec<String>,
}

/// The action a classifier recommends. Any combination may be present; the
/// reactor branches ONLY on these generic fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStateAction {
    /// Key names to inject, e.g. `["down", "enter"]` — same vocabulary as
    /// `codeterm send --actions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
    /// A resume to schedule for later (rate-limit recovery).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delayed_resume: Option<DelayedResume>,
    /// A user-facing surface (questions / limit notice / auth prompt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_update: Option<ProviderUpdatePromptAction>,
}

/// The generic classification verdict returned by a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStateClassification {
    pub state: TabState,
    /// Ensemble vote score in `0.0..=1.0`.
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<TabStateData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<TabStateAction>,
}
