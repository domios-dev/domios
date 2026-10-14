use std::path::Path;

use crate::transcribe::{TranscribeError, TranscribeRegistry};

/// The agent-facing explanation for a voice file the voice stage could not transcribe.
pub fn untranscribed_note(error: &TranscribeError) -> String {
    match error {
        TranscribeError::NoBackend => {
            "No speech-to-text engine. Run codeterm plugin install transcriber, then open \
             the Transcriber panel, pick a model, and click Install — it downloads and \
             enables itself. Do not build anything by hand."
                .to_string()
        }
        other => format!("transcription unavailable: {other}"),
    }
}

/// Build the agent-facing attachment line; voice attachments are transcribed via the active transcriber (`active_id`).
pub async fn format_attachment_line(
    path: &str,
    kind: &str,
    filename: Option<&str>,
    transcribers: &TranscribeRegistry,
    active_id: &str,
) -> String {
    use codeterm_types::message_markers as mk;
    if kind == "voice" {
        if path.is_empty() {
            return mk::voice(path, None, None);
        }
        return match crate::chatpipeline::run_voice_stage(
            transcribers,
            active_id,
            Path::new(path),
            "",
        )
        .await
        {
            Ok(t) => mk::voice(path, Some(&t.text), None),
            Err(e) => {
                if !matches!(e, TranscribeError::NoBackend) {
                    log::warn!("[voice] transcribe failed for {}: {}", path, e);
                }
                mk::voice(path, None, Some(&untranscribed_note(&e)))
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
        assert_eq!(line, "<domios type=\"attachment\" kind=\"voice\" path=\"/note.oga\" label=\"voice\" />\n<transcript>hello world</transcript>");
        let parsed = codeterm_types::message_markers::parse(&line).unwrap();
        assert_eq!(parsed.payload, "hello world");
    }

    // No active transcriber ⇒ the agent-actionable install hint, not a panic.
    #[tokio::test]
    async fn voice_attachment_no_transcriber_offers_install() {
        let reg = TranscribeRegistry::new();
        let line = format_attachment_line("/note.oga", "voice", None, &reg, "missing").await;
        let parsed = codeterm_types::message_markers::parse(&line).unwrap();
        assert_eq!(parsed.attr("status"), Some("untranscribed"));
        assert_eq!(
            parsed.payload,
            untranscribed_note(&TranscribeError::NoBackend)
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
