use std::future::Future;

use super::leases::Capture;

pub(crate) async fn complete_voice_capture(
    capture: &Capture,
    path: &str,
    transcribers: &crate::transcribe::TranscribeRegistry,
    active_id: &str,
    recheck: impl Fn() -> Result<(), String>,
) -> Result<String, String> {
    complete_capture(
        capture,
        async {
            let frame = crate::bridge::voice_transcribe::format_attachment_line(
                path,
                "voice",
                None,
                transcribers,
                active_id,
            )
            .await;
            if let Some(marker) = codeterm_wire::message_markers::parse(&frame) {
                if marker.attr("status") == Some("untranscribed") {
                    return Err(format!(
                        "Voice transcription failed: {}",
                        marker.payload.trim()
                    ));
                }
            }
            Ok(frame)
        },
        recheck,
        Ok::<String, String>,
    )
    .await
}

pub(crate) async fn complete_capture<T>(
    capture: &Capture,
    transcribe: impl Future<Output = Result<String, String>>,
    recheck: impl Fn() -> Result<(), String>,
    admit: impl FnOnce(String) -> Result<T, String>,
) -> Result<T, String> {
    capture.validate()?;
    recheck()?;
    let transcript = transcribe.await?;
    capture.validate()?;
    recheck()?;
    if transcript.trim().is_empty() {
        return Err("recording_transcription_empty".into());
    }
    admit(transcript)
}

#[cfg(test)]
mod tests;
