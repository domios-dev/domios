//! Voice attachment formatting. Runs the compose-time voice stage via
//! [`chatpipeline::run_voice_stage`](crate::chatpipeline::run_voice_stage) and
//! shapes the `-=-codeterm:voice ...-=-` marker (transcript inline on success,
//! else a note/install offer). The engine lives in the bundled `transcriber` plugin.

use std::path::Path;

use crate::transcribe::{TranscribeError, TranscribeRegistry};

/// Build the agent-facing attachment line; voice attachments are transcribed via the active transcriber (`active_id`).
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
                    "No speech-to-text engine. Run codeterm plugin install transcriber, then open \
                     the Transcriber panel, pick a model, and click Install — it downloads and \
                     enables itself. Do not build anything by hand.",
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
        assert!(line.contains("No speech-to-text engine."), "got {line}");
        assert!(
            line.contains("Run codeterm plugin install transcriber"),
            "got {line}"
        );
        assert!(
            line.contains("Do not build anything by hand."),
            "got {line}"
        );
    }

    // Non-voice attachments are untouched by the voice stage.
    #[tokio::test]
    async fn non_voice_attachment_keeps_kind() {
        let reg = TranscribeRegistry::new();
        let line = format_attachment_line("/pic.png", "photo", Some("pic.png"), &reg, "fake").await;
        assert!(line.contains("photo"), "got {line}");
    }
}
