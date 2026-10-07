//! `JsConverter` — adapts a JS plugin that declares `graphNodes.converters` to
//! the core [`Converter`](crate::converter::Converter) trait. Mirrors
//! `JsTranscriber`: it wraps the shared `Arc<PluginVm>` and dispatches to the
//! plugin's `convert(request)` export via the synchronous `call_json`, mapping
//! the `{text}` | `{path}` | `{error}` responses.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::Deserialize;

use super::vm::{call_json_interruptible, PluginVm};
use crate::converter::{
    output_within, Conversion, ConvertError, ConvertOutput, ConvertRequest, Converter,
};

pub struct JsConverter {
    vm: Arc<PluginVm>,
    conversions: Vec<Conversion>,
}

impl JsConverter {
    pub fn new(vm: Arc<PluginVm>, conversions: Vec<Conversion>) -> Self {
        Self { vm, conversions }
    }
}

#[derive(Deserialize)]
struct ConvertOut {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl Converter for JsConverter {
    fn conversions(&self) -> &[Conversion] {
        &self.conversions
    }

    fn convert(&self, req: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
        if Instant::now() >= req.deadline {
            return Err(ConvertError::DeadlineExceeded);
        }
        if req.cancellation.is_cancelled() {
            return Err(ConvertError::Cancelled);
        }
        let args = [serde_json::json!({
            "inputPath": req.input_path.to_string_lossy().to_string(),
            "from": req.from.as_str(),
            "to": req.to.as_str(),
            "outputDir": req.output_dir.to_string_lossy().to_string(),
            "language": req.language,
            "deadlineMs": req.deadline.saturating_duration_since(Instant::now()).as_millis(),
        })];
        match call_json_interruptible::<ConvertOut>(
            &self.vm,
            "convert",
            &args,
            req.cancellation.clone(),
            req.deadline,
        ) {
            Ok(out) => {
                if let Some(e) = out.error {
                    return Err(ConvertError::RequestFailed(e));
                }
                // A file result must land in the request's output directory.
                if let Some(raw) = out.path {
                    let path = PathBuf::from(raw);
                    if !output_within(&req.output_dir, &path) {
                        return Err(ConvertError::OutputOutsideDir(path));
                    }
                    return Ok(ConvertOutput::File(path));
                }
                if let Some(text) = out.text {
                    return Ok(ConvertOutput::Text(text));
                }
                Err(ConvertError::NoOutput)
            }
            Err(_) if req.cancellation.is_cancelled() => Err(ConvertError::Cancelled),
            Err(_) if Instant::now() >= req.deadline => Err(ConvertError::DeadlineExceeded),
            Err(e) => Err(ConvertError::RequestFailed(format!(
                "plugin convert failed: {e:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
