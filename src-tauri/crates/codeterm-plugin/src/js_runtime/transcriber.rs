use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json_async, PluginVm};
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
        let args = [
            json!(audio_path.to_string_lossy().to_string()),
            json!(language),
        ];
        match call_json_async::<TranscribeOut>(self.vm.clone(), "transcribe", args.to_vec()).await {
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
            Err(e) => Err(TranscribeError::RequestFailed(format!(
                "plugin transcribe failed: {e:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
