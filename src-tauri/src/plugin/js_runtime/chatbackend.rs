//! `JsChatBackend` — adapts a JS plugin that declares `capabilities.chatBackend`
//! to the core [`ChatBackendProvider`](crate::chatbackend::ChatBackendProvider)
//! trait. Mirrors `JsTranscriber`: wraps the shared `Arc<PluginVm>` and
//! dispatches to exported plugin functions via the synchronous `call_json` (the
//! VM is not `Send`, so we call it directly on the caller's thread).
//!
//! Plugin surface (all via `call_json`):
//! * `openSession(ctx) -> { sessionId }` — `ctx` is the SDK
//!   `{ paneId, config, systemPrompt?, preset?, model? }` envelope (the host
//!   builds it; a plugin may key its session on `paneId`).
//! * `sendMessage(sessionId, text)` — fire; transcript arrives via `poll`
//! * `poll(sessionId) -> NormalizedChatMessage[]` (or `{ messages: [...] }`)
//! * `closeSession(sessionId)` — optional, best-effort
//! * `listModels() -> Model[]` (`{ id, displayName, description? }`)
//! * `listPresets() -> Preset[]` (`{ id, name, description? }`) optional

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm, VmError};
use crate::agent::chat_types::ChatMsg;
use crate::agent::provider::ModelInfo;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, PresetInfo};

pub struct JsChatBackend {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsChatBackend {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[derive(Deserialize)]
struct OpenOut {
    #[serde(default, rename = "sessionId", alias = "session_id")]
    session_id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// `poll` may return a bare `NormalizedChatMessage[]` or `{ messages: [...] }`.
#[derive(Deserialize)]
#[serde(untagged)]
enum PollOut {
    List(Vec<ChatMsg>),
    Wrapped {
        #[serde(default)]
        messages: Vec<ChatMsg>,
    },
}

#[derive(Deserialize)]
struct PluginModel {
    id: String,
    #[serde(default, rename = "displayName", alias = "display_name")]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct PluginPreset {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

fn map_err(method: &str, e: super::vm::VmError) -> ChatBackendError {
    ChatBackendError::Call(format!("plugin {method} failed: {e:?}"))
}

impl ChatBackendProvider for JsChatBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn open_session(&self, ctx: serde_json::Value) -> Result<String, ChatBackendError> {
        let out: OpenOut =
            call_json(&self.vm, "openSession", &[ctx]).map_err(|e| map_err("openSession", e))?;
        if let Some(e) = out.error {
            return Err(ChatBackendError::Call(e));
        }
        out.session_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ChatBackendError::Call("openSession returned no sessionId".into()))
    }

    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError> {
        call_json::<serde_json::Value>(&self.vm, "sendMessage", &[json!(session_id), json!(text)])
            .map_err(|e| map_err("sendMessage", e))?;
        Ok(())
    }

    fn pump(&self, session_id: &str) -> Result<(), ChatBackendError> {
        // Optional export; a missing `pump` is not an error (best-effort advance,
        // mirrors `closeSession`). Errors are swallowed so a flaky pump can't kill
        // the host poll loop.
        let _ = call_json::<serde_json::Value>(&self.vm, "pump", &[json!(session_id)]);
        Ok(())
    }

    fn poll(&self, session_id: &str) -> Result<Vec<ChatMsg>, ChatBackendError> {
        let out: PollOut =
            call_json(&self.vm, "poll", &[json!(session_id)]).map_err(|e| map_err("poll", e))?;
        Ok(match out {
            PollOut::List(m) => m,
            PollOut::Wrapped { messages } => messages,
        })
    }

    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError> {
        // Optional export; a missing `closeSession` is not an error.
        let _ = call_json::<serde_json::Value>(&self.vm, "closeSession", &[json!(session_id)]);
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        let models: Vec<PluginModel> =
            call_json(&self.vm, "listModels", &[]).map_err(|e| map_err("listModels", e))?;
        Ok(models
            .into_iter()
            .map(|m| ModelInfo {
                display_name: m.display_name.unwrap_or_else(|| m.id.clone()),
                id: m.id,
                description: m.description,
            })
            .collect())
    }

    fn list_presets(&self) -> Result<Vec<PresetInfo>, ChatBackendError> {
        let presets = match call_json::<Vec<PluginPreset>>(&self.vm, "listPresets", &[]) {
            Ok(presets) => presets,
            Err(VmError::MissingFunction(_)) => return Ok(Vec::new()),
            Err(VmError::Script(msg)) if msg.contains("not a function") => return Ok(Vec::new()),
            Err(e) => return Err(map_err("listPresets", e)),
        };
        Ok(presets
            .into_iter()
            .map(|p| PresetInfo {
                id: p.id,
                name: p.name,
                description: p.description,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-chatbackend", src.to_string()).expect("vm"))
    }

    #[test]
    fn open_session_returns_id() {
        let vm = vm_with(
            r#"module.exports.default = {
                openSession(cfg) { return { sessionId: "s-" + (cfg.model || "x") }; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let sid = b.open_session(json!({ "model": "qwen" })).unwrap();
        assert_eq!(sid, "s-qwen");
    }

    #[test]
    fn open_session_forwards_ctx_pane_id() {
        // The host passes the SDK ctx envelope `{ paneId, config }`; a plugin
        // keying its session on paneId (LM Studio) must receive it. Guards the
        // P1 wiring bug where the host passed bare config and `ctx.paneId` was
        // null.
        let vm = vm_with(
            r#"module.exports.default = {
                openSession(ctx) { return { sessionId: ctx.paneId }; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let sid = b
            .open_session(json!({ "paneId": "pane-7", "config": {} }))
            .unwrap();
        assert_eq!(sid, "pane-7");
    }

    #[test]
    fn open_session_forwards_open_shell_ctx_fields() {
        let vm = vm_with(
            r#"module.exports.default = {
                openSession(ctx) {
                    return {
                        sessionId: [
                            ctx.paneId,
                            ctx.systemPrompt,
                            ctx.preset,
                            ctx.model
                        ].join("|")
                    };
                }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let sid = b
            .open_session(json!({
                "paneId": "pane-7",
                "config": {},
                "systemPrompt": "You are CodeTerm.",
                "preset": "codeterm",
                "model": "qwen"
            }))
            .unwrap();
        assert_eq!(sid, "pane-7|You are CodeTerm.|codeterm|qwen");
    }

    #[test]
    fn open_session_error_propagates() {
        let vm = vm_with(
            r#"module.exports.default = { openSession() { return { error: "no server" }; } }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        assert!(matches!(
            b.open_session(json!({})),
            Err(ChatBackendError::Call(_))
        ));
    }

    #[test]
    fn poll_maps_normalized_messages_type_to_kind() {
        // Plugin returns the SDK shape `{ id, type, content }`; serde maps
        // `type` → ChatMsg.kind via the rename on ChatMsg.
        let vm = vm_with(
            r#"module.exports.default = {
                poll(sid) { return [
                    { id: "1", type: "user", content: "hi" },
                    { id: "2", type: "assistant", content: "yo" }
                ]; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let msgs = b.poll("s1").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].kind, "user");
        assert_eq!(msgs[0].content, "hi");
        assert_eq!(msgs[1].kind, "assistant");
    }

    #[test]
    fn poll_accepts_wrapped_messages_shape() {
        let vm = vm_with(
            r#"module.exports.default = {
                poll() { return { messages: [{ id: "1", type: "assistant", content: "ok" }] }; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let msgs = b.poll("s1").unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "ok");
    }

    #[test]
    fn list_models_maps_camel_display_name() {
        let vm = vm_with(
            r#"module.exports.default = {
                listModels() { return [
                    { id: "qwen", displayName: "Qwen 2.5", description: "local" },
                    { id: "bare" }
                ]; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let models = b.list_models().unwrap();
        assert_eq!(models[0].id, "qwen");
        assert_eq!(models[0].display_name, "Qwen 2.5");
        assert_eq!(models[0].description.as_deref(), Some("local"));
        // Missing displayName falls back to the id.
        assert_eq!(models[1].display_name, "bare");
    }

    #[test]
    fn list_presets_maps_optional_export() {
        let vm = vm_with(
            r#"module.exports.default = {
                listPresets() { return [
                    { id: "codeterm", name: "CodeTerm", description: "default shell" },
                    { id: "rhymes", name: "Rhymes" }
                ]; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        let presets = b.list_presets().unwrap();
        assert_eq!(presets[0].id, "codeterm");
        assert_eq!(presets[0].name, "CodeTerm");
        assert_eq!(presets[0].description.as_deref(), Some("default shell"));
        assert_eq!(presets[1].id, "rhymes");
        assert_eq!(presets[1].name, "Rhymes");
        assert_eq!(presets[1].description, None);
    }

    #[test]
    fn list_presets_missing_export_returns_empty_list() {
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "lmstudio");
        assert!(b.list_presets().unwrap().is_empty());
    }

    #[test]
    fn send_message_dispatches_without_error() {
        let vm = vm_with(
            r#"module.exports.default = {
                sendMessage(sid, text) { globalThis.__last = sid + ":" + text; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "lmstudio");
        assert!(b.send_message("s1", "hello").is_ok());
    }

    #[test]
    fn pump_advances_state_that_poll_then_surfaces() {
        // Item 6: the host calls pump() each tick BEFORE poll(); an async plugin
        // moves completed work into its transcript in pump, poll surfaces it.
        let vm = vm_with(
            r#"let pumped = 0;
               module.exports.default = {
                 pump(sid) { pumped++; },
                 poll(sid) { return pumped > 0
                     ? [{ id: "1", type: "assistant", content: "advanced" }]
                     : []; }
               }"#,
        );
        let b = JsChatBackend::new(vm, "npc");
        assert!(b.poll("s").unwrap().is_empty(), "no messages before pump");
        b.pump("s").unwrap();
        let msgs = b.poll("s").unwrap();
        assert_eq!(msgs.len(), 1, "pump advanced state poll surfaces");
        assert_eq!(msgs[0].content, "advanced");
    }

    #[test]
    fn pump_missing_export_is_ok() {
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "npc");
        assert!(b.pump("s").is_ok(), "a plugin without pump is unaffected");
    }

    #[test]
    fn close_session_missing_export_is_ok() {
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "lmstudio");
        assert!(b.close_session("s1").is_ok());
    }
}
