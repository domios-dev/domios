//! Voice attachment formatting.
//!
//! The transcriber is the reference **compose-time voice stage** of the outbound
//! pipeline (plan §6): it grabs the `voice` slot and turns an audio attachment
//! into editable text. This module runs that stage via
//! [`chatpipeline::run_voice_stage`](crate::chatpipeline::run_voice_stage) and
//! shapes the shared `-=-codeterm:voice ...-=-` marker — transcript inline on
//! success, a note otherwise (including an install offer when no transcriber is
//! active). The stage invocation is centralized in `chatpipeline`; this module
//! owns only the marker shaping, so the editable result and the Telegram-bridge
//! reuse (`bridge::manager`) are preserved unchanged.
//!
//! The transcription engine lives in the bundled `transcriber` plugin.

use std::path::Path;

use crate::transcribe::{TranscribeError, TranscribeRegistry};

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
        return match crate::chatpipeline::run_voice_stage(
            transcribers,
            active_id,
            Path::new(path),
            "",
        )
        .await
        {
            Ok(t) => mk::voice(path, Some(&t.text), None),
            Err(TranscribeError::NoBackend) => mk::voice(
                path,
                None,
                Some(
                    "No speech-to-text engine is installed. Ask the user if they want voice \
                     transcription set up; if yes, run `codeterm plugin install transcriber` \
                     (the codeterm-plugins channel is pre-registered), then offer to configure it.",
                ),
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct FakeTranscriber;

    #[async_trait::async_trait]
    impl crate::transcribe::Transcriber for FakeTranscriber {
        fn id(&self) -> &str {
            "fake"
        }
        async fn status(&self) -> crate::transcribe::TranscriberStatus {
            crate::transcribe::TranscriberStatus::Ready
        }
        async fn transcribe(
            &self,
            _audio_path: &Path,
            _language: &str,
        ) -> Result<crate::transcribe::Transcript, TranscribeError> {
            Ok(crate::transcribe::Transcript {
                text: "hello world".into(),
            })
        }
    }

    // The transcriber recast as a compose-time voice stage still produces the
    // same marker — transcript inline on success.
    #[tokio::test]
    async fn voice_attachment_inlines_transcript() {
        let reg = TranscribeRegistry::new();
        reg.register("fake", Arc::new(FakeTranscriber));
        let line = format_attachment_line("/note.oga", "voice", None, &reg, "fake").await;
        assert!(line.contains("voice"), "got {line}");
        assert!(line.contains("hello world"), "got {line}");
    }

    // No active transcriber ⇒ the agent-actionable install hint, not a panic.
    #[tokio::test]
    async fn voice_attachment_no_transcriber_offers_install() {
        let reg = TranscribeRegistry::new();
        let line = format_attachment_line("/note.oga", "voice", None, &reg, "missing").await;
        assert!(
            line.contains("codeterm plugin install transcriber"),
            "got {line}"
        );
        assert!(line.contains("Ask the user"), "got {line}");
    }

    // Non-voice attachments are untouched by the voice stage.
    #[tokio::test]
    async fn non_voice_attachment_keeps_kind() {
        let reg = TranscribeRegistry::new();
        let line = format_attachment_line("/pic.png", "photo", Some("pic.png"), &reg, "fake").await;
        assert!(line.contains("photo"), "got {line}");
    }
}
