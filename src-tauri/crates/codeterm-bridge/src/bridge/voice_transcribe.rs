use std::path::Path;

use crate::transcribe::{TranscribeError, TranscribeRegistry};

/// The agent-facing explanation for a voice file the voice stage could not transcribe.
pub fn untranscribed_note(error: &TranscribeError) -> String {
    match error {
        TranscribeError::NoBackend => {
            "No speech-to-text engine. Run domios plugin install transcriber, then open \
             the Transcriber panel, pick a model, and click Install — it downloads and \
             enables itself. Do not build anything by hand."
                .to_string()
        }
        TranscribeError::TimedOut(_) => format!(
            "transcription unavailable: {error}. The voice note is kept; send it again to retry."
        ),
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
    use codeterm_wire::message_markers as mk;
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
        let parsed = codeterm_wire::message_markers::parse(&line).unwrap();
        assert_eq!(parsed.payload, "hello world");
    }

    // No active transcriber ⇒ the agent-actionable install hint, not a panic.
    #[tokio::test]
    async fn voice_attachment_no_transcriber_offers_install() {
        let reg = TranscribeRegistry::new();
        let line = format_attachment_line("/note.oga", "voice", None, &reg, "missing").await;
        let parsed = codeterm_wire::message_markers::parse(&line).unwrap();
        assert_eq!(parsed.attr("status"), Some("untranscribed"));
        assert_eq!(
            parsed.payload,
            untranscribed_note(&TranscribeError::NoBackend)
        );
    }

    struct SlowTranscriber;

    #[async_trait::async_trait]
    impl crate::transcribe::Transcriber for SlowTranscriber {
        fn id(&self) -> &str {
            "slow"
        }
        async fn status(&self) -> crate::transcribe::TranscriberStatus {
            crate::transcribe::TranscriberStatus::Ready
        }
        async fn transcribe(
            &self,
            _audio_path: &Path,
            _language: &str,
        ) -> Result<crate::transcribe::Transcript, TranscribeError> {
            Err(TranscribeError::TimedOut(std::time::Duration::from_secs(
                300,
            )))
        }
    }

    // A timed-out transcription keeps the note's path and says honestly what happened.
    #[tokio::test]
    async fn timed_out_voice_attachment_keeps_the_note_and_explains_retry() {
        let reg = TranscribeRegistry::new();
        reg.register("slow", Arc::new(SlowTranscriber));
        let line = format_attachment_line("/note.webm", "voice", None, &reg, "slow").await;
        let parsed = codeterm_wire::message_markers::parse(&line).unwrap();
        assert_eq!(parsed.attr("status"), Some("untranscribed"));
        // The parser exposes the wire `path` attribute as `file`.
        assert_eq!(parsed.attr("file"), Some("/note.webm"));
        assert!(line.contains("path=\"/note.webm\""), "got {line}");
        assert!(parsed.payload.contains("did not finish within 300 s"));
        assert!(parsed.payload.contains("send it again to retry"));
    }

    // Non-voice attachments are untouched by the voice stage.
    #[tokio::test]
    async fn non_voice_attachment_keeps_kind() {
        let reg = TranscribeRegistry::new();
        let line = format_attachment_line("/pic.png", "photo", Some("pic.png"), &reg, "fake").await;
        assert!(line.contains("photo"), "got {line}");
    }
}
