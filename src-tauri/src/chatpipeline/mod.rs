//! Outbound message-stage pipeline. One ordered pipeline of stages, each
//! declaring a [`StageMatch`] slot it grabs (voice = compose-time transcriber,
//! text = send-time `chatPreprocessor`) and passing non-matching messages
//! through. Two stages on the same slot would double-process; [`detect_conflicts`]
//! flags the overlap (warn-and-allow).

use std::sync::Arc;

use crate::chatpreprocessor::{ChatPreprocessorProvider, PreprocessCtx};

/// The slot a stage grabs in the outbound pipeline. Part of the conflict guard:
/// two stages on the same slot overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageMatch {
    /// Compose-time: a voice attachment → editable text (transcriber).
    Voice,
    /// Send-time: final text → augmented text (chatPreprocessor).
    Text,
}

/// A flagged overlap: `slot` is claimed by more than one stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageConflict {
    pub slot: StageMatch,
    pub ids: Vec<String>,
}

/// Inspect every registered `(id, slot)` and flag slots claimed by >1 stage.
/// Called at install/registration; the result is warned (not blocked).
pub fn detect_conflicts<'a, I>(stages: I) -> Vec<StageConflict>
where
    I: IntoIterator<Item = (&'a str, StageMatch)>,
{
    let mut voice: Vec<String> = Vec::new();
    let mut text: Vec<String> = Vec::new();
    for (id, slot) in stages {
        match slot {
            StageMatch::Voice => voice.push(id.to_string()),
            StageMatch::Text => text.push(id.to_string()),
        }
    }
    let mut out = Vec::new();
    if voice.len() > 1 {
        out.push(StageConflict {
            slot: StageMatch::Voice,
            ids: voice,
        });
    }
    if text.len() > 1 {
        out.push(StageConflict {
            slot: StageMatch::Text,
            ids: text,
        });
    }
    out
}

/// Run [`detect_conflicts`] over the live registries and `log::warn!` each overlap (warn-and-allow; never blocks).
pub fn warn_stage_conflicts(
    transcribers: &crate::transcribe::TranscribeRegistry,
    preprocessors: &crate::chatpreprocessor::ChatPreprocessorRegistry,
) {
    let voice = transcribers.ids();
    let text = preprocessors.ids();
    let inventory: Vec<(&str, StageMatch)> = voice
        .iter()
        .map(|id| (id.as_str(), StageMatch::Voice))
        .chain(text.iter().map(|id| (id.as_str(), StageMatch::Text)))
        .collect();
    for c in detect_conflicts(inventory) {
        log::warn!(
            "[chatpipeline] {:?} slot grabbed by >1 stage ({}); they would run \
             twice — uninstall one",
            c.slot,
            c.ids.join(", ")
        );
    }
}

/// Run the send-time text stages in order; each replaces the text or passes through. Identity with no stages.
pub fn run_text_stages(
    stages: &[Arc<dyn ChatPreprocessorProvider>],
    ctx: &PreprocessCtx,
    text: String,
) -> String {
    let mut current = text;
    for stage in stages {
        if let Some(next) = stage.preprocess(ctx, &current) {
            current = next;
        }
    }
    current
}

/// Whether a send at a user-facing seam should run the send-time text stages.
/// Excludes service pushes and `source_pane` deliveries (not user turns),
/// chatBackend panes (their transport already runs the stages — avoid double-run),
/// and shell panes (a `<context>` block would garble `echo X`).
pub fn is_user_turn_send(
    is_service_push: bool,
    has_source_pane: bool,
    is_chat_backed: bool,
    is_agent_pane: bool,
) -> bool {
    is_agent_pane && !is_service_push && !has_source_pane && !is_chat_backed
}

/// Compose-time voice stage: resolve the active transcriber and turn an audio file
/// into editable text. No active transcriber ⇒ [`TranscribeError::NoBackend`].
pub async fn run_voice_stage(
    transcribers: &crate::transcribe::TranscribeRegistry,
    active_id: &str,
    path: &std::path::Path,
    language: &str,
) -> Result<crate::transcribe::Transcript, crate::transcribe::TranscribeError> {
    match resolve_transcriber(transcribers, active_id) {
        Some(engine) => engine.transcribe(path, language).await,
        None => Err(crate::transcribe::TranscribeError::NoBackend),
    }
}

fn resolve_transcriber(
    transcribers: &crate::transcribe::TranscribeRegistry,
    active_id: &str,
) -> Option<std::sync::Arc<dyn crate::transcribe::Transcriber>> {
    if !active_id.is_empty() {
        if let Some(engine) = transcribers.get(active_id) {
            return Some(engine);
        }
    }

    let mut ids = transcribers.ids();
    ids.sort();
    let id = match ids.as_slice() {
        [] => return None,
        [only] => only,
        many => many
            .iter()
            .find(|id| id.as_str() == "transcriber")
            .unwrap_or(&many[0]),
    };
    transcribers.get(id)
}

#[cfg(test)]
mod tests;
