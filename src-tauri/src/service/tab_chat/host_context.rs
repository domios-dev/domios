//! Host-owned context delivery: read the host transcript, compose the turn
//! ([`super::context_window`]), hand it to `send_turn`, and record what was
//! decided in the `.context.jsonl` sidecar — one line per host-composed turn.
//!
//! **This bounds the HOST TURN, not each HTTP request.** The plugin's tool loop
//! re-enters per round-trip, so one host send can be N server calls and only
//! the first is host-composed.
//!
//! The sidecar is written on the SEND side, before the request exists — which
//! is why it is a sidecar and not the poll-written mirror. A plugin-owned
//! session never reaches this module, so its sidecar never exists at all.

use std::io::Write;
use std::path::Path;

use super::chatbackend_history;
use super::context_window::{self, ContextDecision, ContextOutcome};
use crate::chatbackend::ChatBackendProvider;

/// Typed refusal for an assembled payload over the ceiling. The offending entry
/// is named and nothing is truncated.
pub(crate) const PAYLOAD_TOO_LARGE_CODE: &str = "chat_backend_context_payload_too_large";

pub(crate) fn deliver_host_turn_for_session(
    provider: &dyn ChatBackendProvider,
    plugin_id: &str,
    session_id: &str,
    text: &str,
) -> Result<(), String> {
    deliver_host_turn(
        provider,
        session_id,
        &chatbackend_history::session_path(plugin_id, session_id),
        &chatbackend_history::context_sidecar_path(plugin_id, session_id),
        text,
    )
}

pub(crate) fn deliver_host_turn(
    provider: &dyn ChatBackendProvider,
    session_id: &str,
    mirror: &Path,
    sidecar: &Path,
    text: &str,
) -> Result<(), String> {
    let system = system_prompt_of(mirror);
    let (transcript, readable) = read_transcript(mirror);
    let mut composed = context_window::compose(
        &system,
        &transcript,
        context_window::PROVISIONAL_GROUP_LIMIT,
        text,
    );
    if !readable && composed.decision.outcome == ContextOutcome::Sent {
        composed.decision.outcome = ContextOutcome::TranscriptUnavailable;
    }

    if !composed.is_sendable() {
        let error = payload_too_large_error(&composed.decision);
        composed.decision.error = Some(error.clone());
        append_decision(sidecar, composed.decision);
        return Err(error);
    }

    let result = provider
        .send_turn(session_id, composed.turn)
        .map_err(|error| error.to_string());
    if let Err(error) = &result {
        composed.decision.outcome = ContextOutcome::Rejected;
        composed.decision.error = Some(error.clone());
    }
    append_decision(sidecar, composed.decision);
    result
}

/// The role brief the session was opened with, from the mirror's `_meta` header.
fn system_prompt_of(mirror: &Path) -> String {
    chatbackend_history::read_meta(mirror)
        .and_then(|meta| {
            meta.get("systemPrompt")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// `false` ⇒ the mirror exists but could not be read, which degrades loudly
/// through `transcript_unavailable` instead of sending a context-free turn.
fn read_transcript(mirror: &Path) -> (Vec<crate::agent::chat_types::ChatMsg>, bool) {
    match chatbackend_history::try_read_messages(mirror) {
        Ok(messages) => (messages, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), true),
        Err(_) => (Vec::new(), false),
    }
}

fn payload_too_large_error(decision: &ContextDecision) -> String {
    serde_json::json!({
        "code": PAYLOAD_TOO_LARGE_CODE,
        "transport": "chat_backend",
        "entryId": decision.offending_entry_id,
        "payloadBytes": decision.payload_bytes,
        "ceilingBytes": context_window::MAX_PAYLOAD_BYTES,
    })
    .to_string()
}

/// Append one decision line, numbering the turn from the lines already present.
fn append_decision(sidecar: &Path, mut decision: ContextDecision) {
    decision.turn = next_turn(sidecar);
    let Ok(line) = serde_json::to_string(&decision) else {
        return;
    };
    if let Some(parent) = sidecar.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(sidecar)
        .and_then(|mut file| writeln!(file, "{line}"));
    if let Err(error) = appended {
        log::warn!("[chatbackend.context] decision record not written: {error}");
    }
}

fn next_turn(sidecar: &Path) -> u64 {
    std::fs::read_to_string(sidecar)
        .map(|content| {
            content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as u64
        })
        .unwrap_or(0)
        + 1
}

/// Read the decision records a session has accumulated.
pub fn read_decisions(sidecar: &Path) -> Vec<ContextDecision> {
    let Ok(content) = std::fs::read_to_string(sidecar) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<ContextDecision>(line.trim()).ok())
        .collect()
}

#[cfg(test)]
mod tests;
