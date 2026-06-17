//! Voice attachment formatting.
//!
//! Resolves the active [`Transcriber`](crate::transcribe::Transcriber) from the
//! registry and emits the shared `-=-codeterm:voice ...-=-` marker — transcript
//! inline on success, a note otherwise (including an install offer when no
//! transcriber is active). Shared by the chat composer and the Telegram bridge.
//!
//! The transcription engine lives in the bundled `transcriber` plugin; this
//! module is only the attachment-line shaping.

use std::path::Path;

use crate::transcribe::TranscribeRegistry;

/// Build the agent-facing attachment line for a file. Voice attachments are
/// transcribed via the active transcriber (`active_id` = config
/// `integrations.voice_transcribe.active`, resolved in `transcribers`).
pub async fn format_attachment_line(
    path: &str,
    kind: &str,
    filename: Option<&str>,
    transcribers: &TranscribeRegistry,
    active_id: &str,
) -> String {
    use crate::message_markers as mk;
    if kind == "voice" && !path.is_empty() {
        let Some(engine) = transcribers.get(active_id) else {
            return mk::voice(
                path,
                None,
                Some(
                    "no transcriber installed; install the Transcriber plugin from your \
                     channel to transcribe voice notes",
                ),
            );
        };
        return match engine.transcribe(Path::new(path), "").await {
            Ok(t) => mk::voice(path, Some(&t.text), None),
            Err(e) => {
                log::warn!("[voice] transcribe failed for {}: {}", path, e);
                mk::voice(
                    path,
                    None,
                    Some(&format!("transcription unavailable: {}", e)),
                )
            }
        };
    }
    // Non-voice attachment (photo/document/video/…): keep the real kind.
    mk::Marker::new(kind)
        .attr("file", path)
        .attr_opt("name", filename)
        .build()
}
