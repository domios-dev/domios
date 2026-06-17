//! `JsChatPreprocessor` — adapts a JS plugin that declares
//! `capabilities.chatPreprocessor` to the core
//! [`ChatPreprocessorProvider`](crate::chatpreprocessor::ChatPreprocessorProvider)
//! trait. Mirrors `JsTranscriber`/`JsChatBackend`: wraps the shared
//! `Arc<PluginVm>` and dispatches `chatPreprocess(ctx)` via the synchronous
//! `call_json` (the VM is not `Send`, so we call it on the caller's thread).
//!
//! Plugin surface: `chatPreprocess(ctx) -> { text } | null`. The host merges the
//! outbound `text` into `ctx` before the call. `null` (or a throw / bad output)
//! ⇒ passthrough — a broken preprocessor must never drop the user's message.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm};
use crate::chatpreprocessor::{ChatPreprocessorProvider, PreprocessCtx};

pub struct JsChatPreprocessor {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsChatPreprocessor {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[derive(Deserialize)]
struct PreprocessOut {
    #[serde(default)]
    text: Option<String>,
}

impl ChatPreprocessorProvider for JsChatPreprocessor {
    fn id(&self) -> &str {
        &self.id
    }

    fn preprocess(&self, ctx: &PreprocessCtx, text: &str) -> Option<String> {
        // Merge the live `text` into the SDK ctx object.
        let mut ctx_json = serde_json::to_value(ctx).ok()?;
        if let Some(obj) = ctx_json.as_object_mut() {
            obj.insert("text".into(), json!(text));
        }
        // `null` return ⇒ Ok(None) ⇒ passthrough; an error fails safe to None.
        match call_json::<Option<PreprocessOut>>(&self.vm, "chatPreprocess", &[ctx_json]) {
            Ok(out) => out.and_then(|o| o.text),
            Err(e) => {
                log::warn!(
                    "[chatpreprocessor] {} chatPreprocess failed (passthrough): {e:?}",
                    self.id
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-chatpreprocessor", src.to_string()).expect("vm"))
    }

    fn ctx() -> PreprocessCtx {
        PreprocessCtx {
            pane_id: "p1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn preprocess_replaces_text() {
        // The ctx carries the live text; the plugin augments it.
        let vm = vm_with(
            r#"module.exports.default = {
                match: "text",
                chatPreprocess(ctx) { return { text: ctx.text + " [" + ctx.paneId + "]" }; }
            }"#,
        );
        let p = JsChatPreprocessor::new(vm, "pre");
        assert_eq!(p.preprocess(&ctx(), "hi"), Some("hi [p1]".to_string()));
    }

    #[test]
    fn null_return_passes_through() {
        let vm = vm_with(r#"module.exports.default = { chatPreprocess() { return null; } }"#);
        let p = JsChatPreprocessor::new(vm, "pre");
        assert_eq!(p.preprocess(&ctx(), "hi"), None);
    }

    #[test]
    fn throwing_plugin_fails_safe_to_passthrough() {
        let vm = vm_with(
            r#"module.exports.default = { chatPreprocess() { throw new Error("boom"); } }"#,
        );
        let p = JsChatPreprocessor::new(vm, "pre");
        assert_eq!(p.preprocess(&ctx(), "keep me"), None);
    }
}
