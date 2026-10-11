use std::future::Future;

use super::leases::Capture;

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
    let text = transcript.trim();
    if text.is_empty() {
        return Err("recording_transcription_empty".into());
    }
    admit(text.to_string())
}

#[cfg(test)]
mod tests;
