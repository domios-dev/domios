//! Outbound message-stage pipeline (plan §6).
//!
//! **A message is a message.** Sending a voice note is not a separate code path
//! — it is an outbound message carrying a property (a voice attachment) some
//! stage filters on. The outbound path is therefore one ordered pipeline of
//! stages; each stage declares a [`StageMatch`] slot (what it grabs) and acts
//! only on matching messages, passing the rest through untouched.
//!
//! * **Voice slot (compose-time):** the transcriber — a voice attachment →
//!   editable text. Run by `bridge::voice_transcribe::format_attachment_line`
//!   via [`run_voice_stage`] (the result stays editable in the composer, and the
//!   Telegram bridge reuses the same formatter).
//! * **Text slot (send-time):** a `chatPreprocessor` — final text → augmented
//!   text. Run by `ServiceContext::run_outbound_stages` via [`run_text_stages`],
//!   after the text is final, transparently.
//!
//! A voice message chains: the transcriber grabs it first (the preprocessor did
//! not match — no text yet), emits text, then the preprocessor matches that text.
//!
//! **Conflict guard ([`detect_conflicts`]).** Two stages claiming the same slot
//! would double-process the same message. On registering stages, CodeTerm flags
//! the overlap. Policy is **warn-and-allow** (plan §6 open decision resolved to
//! warn): non-overlapping stages (voice + text) chain fine and never collide.

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

/// Build the full slot inventory from the live registries (transcribers grab
/// the `voice` slot, preprocessors the `text` slot), run [`detect_conflicts`],
/// and `log::warn!` each overlap. Called at install/registration — the policy is
/// warn-and-allow, so this only surfaces the collision; it never blocks.
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

/// Run the send-time text stages over `text`, in slice order. Each stage either
/// replaces the text (`Some`) or passes through (`None`). Returns the final
/// text. With no stages this is identity — the same contract Track B shipped.
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

/// Whether a send entering at a user-facing seam (REST `send_text`, the desktop
/// composer's `chat_send_text`) should run the send-time text stages (F3, P3).
///
/// A **user turn** is one that is NOT a service push (`svc_token`) and carries
/// NO `source_pane` — inter-pane delivery, mesh relay, and hierarchy reports all
/// set `source_pane`, and orchestrator/restart/GA system sends go through the
/// synchronous `send_text` (never this seam), so the gate excludes them.
///
/// chatBackend panes are excluded here because their transport already runs the
/// stages (`pane_transport_chat` / the chat branch of `pty_io::send_text`) — a
/// second run at this seam would double-process the message.
///
/// `is_agent_pane` is REQUIRED: the stages only make sense delivering to an AI
/// agent. A plain SHELL pane must never get a `<context>` block injected — that
/// garbles `echo X` and broke 23 e2e tests (BUG A regression). Shell panes (no
/// detected `agent_type`) are therefore excluded.
pub fn is_user_turn_send(
    is_service_push: bool,
    has_source_pane: bool,
    is_chat_backed: bool,
    is_agent_pane: bool,
) -> bool {
    is_agent_pane && !is_service_push && !has_source_pane && !is_chat_backed
}

/// Run the compose-time voice stage: resolve the active transcriber and turn an
/// audio file into editable text. The transcriber is the reference voice stage
/// (plan §6) — `format_attachment_line` calls this, then shapes the marker, so
/// the editable result and Telegram-bridge reuse are preserved. No active
/// transcriber ⇒ [`TranscribeError::NoBackend`].
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
