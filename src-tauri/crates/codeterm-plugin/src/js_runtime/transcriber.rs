use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::call_deadline::LongCall;
use super::vm::{call_json_async, call_json_async_until, PluginVm, VmError};
use crate::transcribe::{TranscribeError, Transcriber, TranscriberStatus, Transcript};

pub struct JsTranscriber {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsTranscriber {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[derive(Deserialize)]
struct TranscribeOut {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct StatusOut {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl Transcriber for JsTranscriber {
    fn id(&self) -> &str {
        &self.id
    }

    async fn status(&self) -> TranscriberStatus {
        // `transcribeStatus` is optional; absent/error ⇒ assume Ready.
        match call_json_async::<StatusOut>(self.vm.clone(), "transcribeStatus", Vec::new()).await {
            Ok(s) => match s.state.as_deref() {
                Some("loading") => TranscriberStatus::Loading,
                Some("unloaded") => TranscriberStatus::Unloaded,
                Some("unavailable") => TranscriberStatus::Unavailable {
                    reason: s.reason.unwrap_or_default(),
                },
                _ => TranscriberStatus::Ready,
            },
            Err(_) => TranscriberStatus::Ready,
        }
    }

    async fn transcribe(
        &self,
        audio_path: &Path,
        language: &str,
    ) -> Result<Transcript, TranscribeError> {
        let args = vec![
            json!(audio_path.to_string_lossy().to_string()),
            json!(language),
        ];
        let call = LongCall::Transcription;
        let (cancellation, deadline) = call.interrupt();
        let result = call_json_async_until::<TranscribeOut>(
            self.vm.clone(),
            "transcribe",
            args,
            (cancellation, deadline),
        )
        .await;
        match result {
            Ok(out) => {
                if let Some(e) = out.error {
                    return Err(TranscribeError::RequestFailed(e));
                }
                let text = out.text.unwrap_or_default().trim().to_string();
                if text.is_empty() {
                    Err(TranscribeError::Empty)
                } else {
                    Ok(Transcript { text })
                }
            }
            Err(error) => Err(call_failure(
                error,
                std::time::Instant::now() >= deadline,
                call.bound(),
            )),
        }
    }
}

/// A deadline close revokes the call, so any interruption past the deadline is a timeout.
fn call_failure(
    error: VmError,
    past_deadline: bool,
    bound: std::time::Duration,
) -> TranscribeError {
    match error {
        VmError::AsyncJobTimedOut | VmError::CallCancelled | VmError::IoUnconfirmed
            if past_deadline =>
        {
            TranscribeError::TimedOut(bound)
        }
        error => TranscribeError::RequestFailed(format!("plugin transcribe failed: {error}")),
    }
}

#[cfg(test)]
mod deadline_tests;
#[cfg(test)]
mod tests;
