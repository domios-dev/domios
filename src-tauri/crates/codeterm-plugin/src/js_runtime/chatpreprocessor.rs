//! `JsChatPreprocessor` — adapts a JS `capabilities.chatPreprocessor` plugin to the
//! core [`ChatPreprocessorProvider`](crate::chatpreprocessor::ChatPreprocessorProvider)
//! trait via the synchronous `call_json`. Surface: `chatPreprocess(ctx) -> { text } | null`
//! (host merges `text` into `ctx`); `null` / throw ⇒ passthrough (never drops the message).

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
            tab_id: "p1".into(),
            ..Default::default()
        }
    }

    // ── contract tests (inline JS) ──────────────────────────────────────────

    #[test]
    fn preprocess_replaces_text() {
        let vm = vm_with(
            r#"module.exports.default = {
                match: "text",
                chatPreprocess(ctx) { return { text: ctx.text + " [" + ctx.tabId + "]" }; }
            }"#,
        );
        let p = JsChatPreprocessor::new(vm, "pre");
        assert_eq!(p.preprocess(&ctx(), "hi"), Some("hi [p1]".to_string()));
    }

    #[test]
    fn ctx_fields_cross_the_boundary_camelcase() {
        let vm = vm_with(
            r#"module.exports.default = {
                chatPreprocess(ctx) {
                    return { text: [ctx.provider, ctx.cwd, ctx.turnIndex, ctx.tabId, ctx.text].join("|") };
                }
            }"#,
        );
        let p = JsChatPreprocessor::new(vm, "pre");
        let ctx = PreprocessCtx {
            tab_id: "p1".into(),
            cwd: Some("/work/repo".into()),
            provider: Some("claude".into()),
            turn_index: 7,
        };
        assert_eq!(
            p.preprocess(&ctx, "hello"),
            Some("claude|/work/repo|7|p1|hello".to_string())
        );
    }

    #[test]
    fn optional_ctx_fields_serialize_as_null() {
        let vm = vm_with(
            r#"module.exports.default = {
                chatPreprocess(ctx) {
                    var cwd = ctx.cwd === null ? "NULL" : String(ctx.cwd);
                    var provider = ctx.provider === null ? "NULL" : String(ctx.provider);
                    return { text: provider + "|" + cwd + "|" + ctx.turnIndex };
                }
            }"#,
        );
        let p = JsChatPreprocessor::new(vm, "pre");
        assert_eq!(p.preprocess(&ctx(), "x"), Some("NULL|NULL|0".to_string()));
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

    // ── bundled plugin preset tests ─────────────────────────────────────────

    fn bundled_plugin_js() -> Option<String> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/chat-preprocessor/plugin.js");
        std::fs::read_to_string(&path).ok()
    }

    fn bundled_vm(settings_json: &str) -> Option<Arc<PluginVm>> {
        let src = bundled_plugin_js()?;
        let vm = Arc::new(PluginVm::from_source("chat-preprocessor", src).expect("bundled vm"));
        vm.reload_settings(settings_json.to_string());
        Some(vm)
    }

    #[test]
    fn settings_disabled_always_passes_through() {
        // `enabled: false` (or absent) must short-circuit to null regardless of preset.
        let Some(vm) = bundled_vm(r#"{"enabled":false,"activePreset":"mem-recall"}"#) else {
            return;
        };
        let p = JsChatPreprocessor::new(vm, "chat-preprocessor");
        assert_eq!(
            p.preprocess(&ctx(), "hello"),
            None,
            "disabled → passthrough"
        );
    }

    #[test]
    fn settings_default_null_passes_through() {
        // settings_json starts as "null" until a config.yaml or config.plugins entry is written;
        // the plugin must treat that as disabled and never modify the text.
        let Some(vm) = bundled_vm("null") else { return };
        let p = JsChatPreprocessor::new(vm, "chat-preprocessor");
        assert_eq!(
            p.preprocess(&ctx(), "hello"),
            None,
            "null settings → passthrough"
        );
    }

    #[test]
    fn template_preset_active_applies_replacement() {
        // A user template preset with {{text}} must wrap the outbound message.
        let settings = serde_json::json!({
            "enabled": true,
            "activePreset": "wrap",
            "presets": [{"id": "wrap", "kind": "template", "template": "<<{{text}}>>" }]
        });
        let Some(vm) = bundled_vm(&settings.to_string()) else {
            return;
        };
        let p = JsChatPreprocessor::new(vm, "chat-preprocessor");
        assert_eq!(
            p.preprocess(&ctx(), "hello"),
            Some("<<hello>>".to_string()),
            "template preset must substitute {{text}}"
        );
    }

    #[test]
    fn mem_recall_without_service_context_passes_through() {
        let settings = serde_json::json!({
            "enabled": true,
            "activePreset": "mem-recall"
        });
        let Some(vm) = bundled_vm(&settings.to_string()) else {
            return;
        };
        let p = JsChatPreprocessor::new(vm, "chat-preprocessor");
        assert_eq!(
            p.preprocess(&ctx(), "hello"),
            None,
            "empty mem → no block → text unchanged → passthrough null"
        );
    }

    #[test]
    fn missing_user_preset_id_passes_through() {
        // must return null (nothing to apply) rather than crashing or outputting garbage.
        let settings = serde_json::json!({
            "enabled": true,
            "activePreset": "no-such-preset",
            "presets": []
        });
        let Some(vm) = bundled_vm(&settings.to_string()) else {
            return;
        };
        let p = JsChatPreprocessor::new(vm, "chat-preprocessor");
        assert_eq!(
            p.preprocess(&ctx(), "hello"),
            None,
            "missing preset → passthrough null"
        );
    }
}
