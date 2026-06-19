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
use super::PluginManifest;
use crate::agent::chat_types::{ChatMsg, StructuredChat};
use crate::agent::provider::ModelInfo;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatSessionInfo, PresetInfo};

pub struct JsChatBackend {
    vm: Arc<PluginVm>,
    id: String,
    display_name: String,
    icon_hint: Option<String>,
    icon_svg: Option<String>,
    icon_color: Option<String>,
    /// `capabilities.chatBackend` was authored as `{ spawnable: true }` — the
    /// backend is user-openable from the new-session picker. Bare
    /// `chatBackend: true` ⇒ false (backend-only).
    spawnable: bool,
}

impl JsChatBackend {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            vm,
            display_name: id.clone(),
            id,
            icon_hint: None,
            icon_svg: None,
            icon_color: None,
            spawnable: false,
        }
    }

    pub fn from_manifest(vm: Arc<PluginVm>, manifest: &PluginManifest) -> Self {
        Self {
            vm,
            id: manifest.id.clone(),
            display_name: manifest.display_name.clone(),
            icon_hint: manifest.icon_hint.clone(),
            icon_svg: manifest.icon_svg.clone(),
            icon_color: manifest.icon_color.clone(),
            spawnable: manifest.capabilities.chat_backend_spawnable(),
        }
    }
}

#[derive(Deserialize)]
struct OpenOut {
    #[serde(default, rename = "sessionId", alias = "session_id")]
    session_id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// `poll` may return a bare `NormalizedChatMessage[]` (legacy/cursorless) or the
/// cursor-delta envelope `{ messages, cursor }`.
#[derive(Deserialize)]
#[serde(untagged)]
enum PollOut {
    List(Vec<ChatMsg>),
    Wrapped {
        #[serde(default)]
        messages: Vec<ChatMsg>,
        #[serde(default)]
        cursor: Option<String>,
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

#[derive(Deserialize)]
struct PluginSessionInfo {
    #[serde(default)]
    model: Option<String>,
}

fn map_err(method: &str, e: super::vm::VmError) -> ChatBackendError {
    ChatBackendError::Call(format!("plugin {method} failed: {e:?}"))
}

impl ChatBackendProvider for JsChatBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn spawnable(&self) -> bool {
        self.spawnable
    }

    fn icon_hint(&self) -> Option<&str> {
        self.icon_hint.as_deref()
    }

    fn icon_svg(&self) -> Option<&str> {
        self.icon_svg.as_deref()
    }

    fn icon_color(&self) -> Option<&str> {
        self.icon_color.as_deref()
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

    fn poll(
        &self,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StructuredChat, ChatBackendError> {
        let out: PollOut = call_json(&self.vm, "poll", &[json!(session_id), json!(cursor)])
            .map_err(|e| map_err("poll", e))?;
        Ok(match out {
            PollOut::List(messages) => StructuredChat {
                messages,
                cursor: None,
            },
            PollOut::Wrapped { messages, cursor } => StructuredChat { messages, cursor },
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

    fn session_info(&self, session_id: &str) -> Result<ChatSessionInfo, ChatBackendError> {
        // Optional export — a bare backend with no `sessionInfo` reports no model
        // (honest null), mirroring the `listPresets` missing-export handling.
        let info =
            match call_json::<PluginSessionInfo>(&self.vm, "sessionInfo", &[json!(session_id)]) {
                Ok(info) => info,
                Err(VmError::MissingFunction(_)) => return Ok(ChatSessionInfo::default()),
                Err(VmError::Script(msg)) if msg.contains("not a function") => {
                    return Ok(ChatSessionInfo::default())
                }
                Err(e) => return Err(map_err("sessionInfo", e)),
            };
        Ok(ChatSessionInfo { model: info.model })
    }

    fn set_model(&self, session_id: &str, model: &str) -> Result<(), ChatBackendError> {
        // Required for a switch — a missing export is surfaced as an error so the
        // command reports "unsupported" rather than silently no-op'ing.
        match call_json::<serde_json::Value>(
            &self.vm,
            "setModel",
            &[json!(session_id), json!(model)],
        ) {
            Ok(_) => Ok(()),
            Err(VmError::MissingFunction(_)) => Err(ChatBackendError::Call(
                "this chatBackend does not support switching the model".into(),
            )),
            Err(VmError::Script(msg)) if msg.contains("not a function") => {
                Err(ChatBackendError::Call(
                    "this chatBackend does not support switching the model".into(),
                ))
            }
            Err(e) => Err(map_err("setModel", e)),
        }
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
        let b = JsChatBackend::new(vm, "local-shell");
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
        let b = JsChatBackend::new(vm, "local-shell");
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
        let b = JsChatBackend::new(vm, "local-shell");
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
        let b = JsChatBackend::new(vm, "local-shell");
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
        let b = JsChatBackend::new(vm, "local-shell");
        let msgs = b.poll("s1", None).unwrap().messages;
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
        let b = JsChatBackend::new(vm, "local-shell");
        let msgs = b.poll("s1", None).unwrap().messages;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "ok");
    }

    #[test]
    fn poll_forwards_cursor_and_returns_delta_plus_next_cursor() {
        // Contract: poll(sessionId, cursor) -> { messages, cursor }. The plugin
        // echoes the cursor it received and returns the next one; an idle poll
        // (cursor already at the end) returns an empty delta. Guards the C1+M1
        // wiring bug where the host passed no cursor and got the whole transcript
        // every tick.
        let vm = vm_with(
            r#"module.exports.default = {
                poll(sid, cursor) {
                    if (cursor === "c1") { return { messages: [], cursor: "c1" }; }
                    return {
                        messages: [{ id: "1", type: "assistant", content: cursor || "fresh" }],
                        cursor: "c1"
                    };
                }
            }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");

        // First poll: no cursor ⇒ full delta + seeded cursor.
        let first = b.poll("s1", None).unwrap();
        assert_eq!(first.messages.len(), 1);
        assert_eq!(first.messages[0].content, "fresh");
        assert_eq!(first.cursor.as_deref(), Some("c1"));

        // Next poll passes that cursor ⇒ empty delta (nothing new), cursor held.
        let second = b.poll("s1", Some("c1")).unwrap();
        assert!(second.messages.is_empty(), "idle poll yields no delta");
        assert_eq!(second.cursor.as_deref(), Some("c1"));
    }

    #[test]
    fn poll_bare_list_has_no_cursor() {
        // A legacy/cursorless plugin returning a bare array maps to messages with
        // cursor None (the loop keeps its prior cursor).
        let vm = vm_with(
            r#"module.exports.default = {
                poll(sid, cursor) { return [{ id: "1", type: "assistant", content: "x" }]; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        let out = b.poll("s1", None).unwrap();
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.cursor, None);
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
        let b = JsChatBackend::new(vm, "local-shell");
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
        let b = JsChatBackend::new(vm, "local-shell");
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
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.list_presets().unwrap().is_empty());
    }

    #[test]
    fn send_message_dispatches_without_error() {
        let vm = vm_with(
            r#"module.exports.default = {
                sendMessage(sid, text) { globalThis.__last = sid + ":" + text; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
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
        assert!(
            b.poll("s", None).unwrap().messages.is_empty(),
            "no messages before pump"
        );
        b.pump("s").unwrap();
        let msgs = b.poll("s", None).unwrap().messages;
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
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.close_session("s1").is_ok());
    }

    #[test]
    fn set_model_then_session_info_round_trips_via_exports() {
        // P1 contract: switch_model routes to the plugin's `setModel` export and
        // `session_info` reads the live model back via `sessionInfo`. Mirrors the
        // lmstudio shell where the session model drives the next /api/v1/chat.
        let vm = vm_with(
            r#"let model = "init";
               module.exports.default = {
                 setModel(sid, m) { model = m; },
                 sessionInfo(sid) { return { model }; }
               }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        assert_eq!(b.session_info("s").unwrap().model.as_deref(), Some("init"));
        b.set_model("s", "qwen-2.5").unwrap();
        assert_eq!(
            b.session_info("s").unwrap().model.as_deref(),
            Some("qwen-2.5"),
            "setModel must update the model session_info surfaces"
        );
    }

    #[test]
    fn session_info_missing_export_is_empty() {
        // A bare backend with no `sessionInfo` export reports no model (honest
        // null), never an error.
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.session_info("s").unwrap().model.is_none());
    }

    #[test]
    fn set_model_missing_export_errors() {
        // A backend that cannot switch surfaces an error so the command reports
        // "unsupported" rather than silently doing nothing.
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.set_model("s", "m").is_err());
    }
}
