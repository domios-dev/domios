//! `JsChatBackend` — adapts a JS `capabilities.chatBackend` plugin to the core
//! [`ChatBackendProvider`](crate::chatbackend_provider::ChatBackendProvider) trait via the
//! synchronous `call_json` (the VM is not `Send`, so it's called on the caller's thread).
//! Plugin surface: `openSession` / `sendMessage` / `watcherTick` / `poll` / `closeSession` / `listModels` / `listPresets`.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm, VmError};
use super::PluginManifest;
use crate::chatbackend_provider::{
    ChatBackendError, ChatBackendProvider, ChatSessionInfo, HostTurn, PresetInfo,
    CONTEXT_OWNER_UNSUPPORTED,
};
use crate::provider::{ContextOwner, ModelInfo};
use codeterm_types::agent::chat_types::{ChatMsg, StructuredChat};

pub struct JsChatBackend {
    vm: Arc<PluginVm>,
    id: String,
    display_name: String,
    icon_hint: Option<String>,
    icon_svg: Option<String>,
    icon_color: Option<String>,
    /// `{ spawnable: true }` ⇒ user-openable from the picker; bare `chatBackend: true` ⇒ false (backend-only).
    spawnable: bool,
    watcher_capable: bool,
    session_config: bool,
    context_owner: ContextOwner,
    session_exec_env: parking_lot::Mutex<HashMap<String, HashMap<String, String>>>,
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
            watcher_capable: false,
            session_config: false,
            context_owner: ContextOwner::Plugin,
            session_exec_env: parking_lot::Mutex::new(HashMap::new()),
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
            watcher_capable: manifest.capabilities.chat_backend_watcher_capable(),
            session_config: manifest.capabilities.chat_backend.session_config(),
            context_owner: manifest.capabilities.chat_backend.context_owner(),
            session_exec_env: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    fn exec_env_scope(&self, session_id: &str) -> super::host_exec::HostExecEnvGuard {
        let env = self
            .session_exec_env
            .lock()
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        super::host_exec::host_env_scope(env)
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
        #[serde(default)]
        done: Option<bool>,
    },
}

#[derive(Deserialize)]
struct PluginModel {
    id: String,
    #[serde(default, rename = "displayName", alias = "display_name")]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    available: Option<bool>,
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

fn chatbackend_host_exec_env(
    vm: &Arc<PluginVm>,
    ctx: &serde_json::Value,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if let Some(tab_id) = ctx.get("tabId").and_then(|v| v.as_str()) {
        env.insert("CODETERM_TAB_ID".to_string(), tab_id.to_string());
    }
    if let Some(workspace_id) = ctx.get("workspaceId").and_then(|v| v.as_str()) {
        env.insert(
            "CODETERM_WORKSPACE_ID".to_string(),
            workspace_id.to_string(),
        );
    }
    if let Some(pad_dir) = ctx.get("padDir").and_then(|v| v.as_str()) {
        env.insert("CODETERM_PAD_DIR".to_string(), pad_dir.to_string());
    }
    let socket_path = vm
        .service()
        .map(|svc| svc.socket_display_path())
        .unwrap_or_default();
    env.insert("CODETERM_SOCKET_PATH".to_string(), socket_path);
    if let Some(svc) = vm.service() {
        let api_url = svc.api_base_url();
        let port = api_url.rsplit(':').next().unwrap_or("7685").to_string();
        env.insert("CODETERM_API_URL".to_string(), api_url);
        env.insert("CODETERM_API_PORT".to_string(), port);
        if let Some(token) = svc.api_token() {
            env.insert("CODETERM_API_TOKEN".to_string(), token);
        }
    }
    env
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

    fn watcher_capable(&self) -> bool {
        self.watcher_capable
    }

    fn supports_session_config(&self) -> bool {
        self.session_config
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
        let exec_env = chatbackend_host_exec_env(&self.vm, &ctx);
        let out: OpenOut =
            call_json(&self.vm, "openSession", &[ctx]).map_err(|e| map_err("openSession", e))?;
        if let Some(e) = out.error {
            return Err(ChatBackendError::Call(e));
        }
        let session_id = out
            .session_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ChatBackendError::Call("openSession returned no sessionId".into()))?;
        self.session_exec_env
            .lock()
            .insert(session_id.clone(), exec_env);
        Ok(session_id)
    }

    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        call_json::<serde_json::Value>(&self.vm, "sendMessage", &[json!(session_id), json!(text)])
            .map_err(|e| map_err("sendMessage", e))?;
        Ok(())
    }

    fn context_owner(&self) -> ContextOwner {
        self.context_owner
    }

    fn supports_send_turn(&self) -> bool {
        super::vm::has_method(&self.vm, "sendTurn")
    }

    fn send_turn(&self, session_id: &str, turn: HostTurn) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        let turn = serde_json::to_value(&turn)
            .map_err(|e| ChatBackendError::Call(format!("sendTurn payload failed: {e}")))?;
        match call_json::<serde_json::Value>(&self.vm, "sendTurn", &[json!(session_id), turn]) {
            Ok(_) => Ok(()),
            Err(VmError::MissingFunction(_)) => {
                Err(ChatBackendError::Call(CONTEXT_OWNER_UNSUPPORTED.into()))
            }
            Err(VmError::Script(msg)) if msg.contains("not a function") => {
                Err(ChatBackendError::Call(CONTEXT_OWNER_UNSUPPORTED.into()))
            }
            Err(e) => Err(map_err("sendTurn", e)),
        }
    }

    fn pump(&self, session_id: &str) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        // Optional export, best-effort: errors are swallowed so a flaky pump can't kill the host poll loop.
        let _ = call_json::<serde_json::Value>(&self.vm, "pump", &[json!(session_id)]);
        Ok(())
    }

    fn poll(
        &self,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StructuredChat, ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        let out: PollOut = call_json(&self.vm, "poll", &[json!(session_id), json!(cursor)])
            .map_err(|e| map_err("poll", e))?;
        Ok(match out {
            PollOut::List(messages) => StructuredChat {
                messages,
                cursor: None,
                // Legacy list pollers do not advertise busy state, so watcher arming is host-gated only.
                done: None,
            },
            PollOut::Wrapped {
                messages,
                cursor,
                done,
            } => StructuredChat {
                messages,
                cursor,
                done,
            },
        })
    }

    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        // Optional export; a missing `closeSession` is not an error.
        let _ = call_json::<serde_json::Value>(&self.vm, "closeSession", &[json!(session_id)]);
        self.session_exec_env.lock().remove(session_id);
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        let models: Vec<PluginModel> = match call_json(&self.vm, "listModels", &[]) {
            Ok(models) => models,
            Err(VmError::MissingFunction(_)) => return Ok(Vec::new()),
            Err(VmError::Script(msg)) if msg.contains("not a function") => return Ok(Vec::new()),
            Err(error) => return Err(map_err("listModels", error)),
        };
        Ok(models
            .into_iter()
            .map(|m| ModelInfo {
                display_name: m.display_name.unwrap_or_else(|| m.id.clone()),
                id: m.id,
                description: m.description,
                available: m.available,
                availability: None,
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
        let _guard = self.exec_env_scope(session_id);
        // Optional export — a bare backend with no `sessionInfo` reports no model.
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
        let _guard = self.exec_env_scope(session_id);
        // Required for a switch — a missing export surfaces as an error so the command reports "unsupported".
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

    fn watcher_tick(
        &self,
        session_id: &str,
        input: serde_json::Value,
    ) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        match call_json::<serde_json::Value>(&self.vm, "watcherTick", &[json!(session_id), input]) {
            Ok(_) => Ok(()),
            Err(VmError::MissingFunction(_)) => Err(ChatBackendError::Call(
                "this chatBackend does not support watcher mode".into(),
            )),
            Err(VmError::Script(msg)) if msg.contains("not a function") => Err(
                ChatBackendError::Call("this chatBackend does not support watcher mode".into()),
            ),
            Err(e) => Err(map_err("watcherTick", e)),
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
    fn open_session_forwards_ctx_tab_id() {
        // A plugin keying its session on tabId must receive the full `{ tabId, config }` envelope, not bare config.
        let vm = vm_with(
            r#"module.exports.default = {
                openSession(ctx) { return { sessionId: ctx.tabId }; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        let sid = b
            .open_session(json!({ "tabId": "tab-7", "config": {} }))
            .unwrap();
        assert_eq!(sid, "tab-7");
    }

    #[test]
    fn chatbackend_host_exec_env_includes_tab_identity() {
        let vm = Arc::new(
            PluginVm::from_source(
                "test-chatbackend-env",
                "module.exports.default = {};".into(),
            )
            .unwrap(),
        );
        let env = chatbackend_host_exec_env(
            &vm,
            &json!({
                "tabId": "chat-tab",
                "workspaceId": "ws-1",
                "padDir": "/instance/runtime/agent-pads/chat-tab",
                "config": {}
            }),
        );

        assert_eq!(
            env.get("CODETERM_TAB_ID").map(String::as_str),
            Some("chat-tab")
        );
        assert_eq!(
            env.get("CODETERM_WORKSPACE_ID").map(String::as_str),
            Some("ws-1")
        );
        assert_eq!(
            env.get("CODETERM_PAD_DIR").map(String::as_str),
            Some("/instance/runtime/agent-pads/chat-tab")
        );
        assert!(env.contains_key("CODETERM_SOCKET_PATH"));
    }

    #[test]
    fn send_message_host_exec_sees_chatbackend_tab_identity() {
        let vm = vm_with(
            r#"let last = "";
               module.exports.default = {
                 openSession(ctx) { return { sessionId: "s1" }; },
                 sendMessage(sid, text) {
                   const opts = host.platform() === "windows"
                     ? { bin: "powershell", args: ["-NoProfile", "-Command", "[Console]::Out.Write($env:CODETERM_TAB_ID)"] }
                     : { bin: "sh", args: ["-c", "printf %s \"$CODETERM_TAB_ID\""] };
                   const out = JSON.parse(host.exec(JSON.stringify(opts)));
                   last = out.stdout || "";
                 },
                 poll() { return [{ id: "1", type: "assistant", content: last }]; }
               }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        let sid = b
            .open_session(json!({ "tabId": "chat-tab", "config": {} }))
            .unwrap();
        b.send_message(&sid, "reply").unwrap();

        let msgs = b.poll(&sid, None).unwrap().messages;

        assert_eq!(msgs[0].content, "chat-tab");
    }

    #[test]
    fn open_session_forwards_open_shell_ctx_fields() {
        let vm = vm_with(
            r#"module.exports.default = {
                openSession(ctx) {
                    return {
                        sessionId: [
                            ctx.tabId,
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
                "tabId": "tab-7",
                "config": {},
                "systemPrompt": "You are CodeTerm.",
                "preset": "codeterm",
                "model": "qwen"
            }))
            .unwrap();
        assert_eq!(sid, "tab-7|You are CodeTerm.|codeterm|qwen");
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
        // Plugin returns `{ id, type, content }`; serde maps `type` → ChatMsg.kind.
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
        // Contract: poll(sessionId, cursor) -> { messages, cursor } returns only the delta after the cursor, not the whole transcript every tick.
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
                    { id: "qwen", displayName: "Qwen 2.5", description: "local", available: false },
                    { id: "bare" }
                ]; }
            }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        let models = b.list_models().unwrap();
        assert_eq!(models[0].id, "qwen");
        assert_eq!(models[0].display_name, "Qwen 2.5");
        assert_eq!(models[0].description.as_deref(), Some("local"));
        assert_eq!(models[0].available, Some(false));
        // Missing displayName falls back to the id.
        assert_eq!(models[1].display_name, "bare");
    }

    #[test]
    fn list_models_missing_export_returns_empty_list() {
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.list_models().unwrap().is_empty());
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
    fn watcher_tick_forwards_session_and_input() {
        let vm = vm_with(
            r#"let last = "";
               module.exports.default = {
                 watcherTick(sid, input) { last = JSON.stringify({ sid, input }); },
                 poll() { return [{ id: "1", type: "assistant", content: last }]; }
               }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        let input = json!({
            "tick": 3,
            "nowMs": 123456,
            "state": { "round": 2 },
            "observations": { "tabs": ["orch", "worker"] }
        });

        b.watcher_tick("watch-1", input.clone()).unwrap();
        let msgs = b.poll("watch-1", None).unwrap().messages;
        let forwarded: serde_json::Value = serde_json::from_str(&msgs[0].content).unwrap();

        assert_eq!(forwarded, json!({ "sid": "watch-1", "input": input }));
    }

    #[test]
    fn watcher_tick_missing_export_errors_unsupported() {
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        let err = b.watcher_tick("s1", json!({ "tick": 1 })).unwrap_err();

        assert!(err
            .to_string()
            .contains("this chatBackend does not support watcher mode"));
    }

    #[test]
    fn send_turn_forwards_session_and_composed_turn() {
        let vm = vm_with(
            r#"let last = "";
               module.exports.default = {
                 sendTurn(sid, turn) { last = JSON.stringify({ sid, turn }); },
                 poll() { return [{ id: "1", type: "assistant", content: last }]; }
               }"#,
        );
        let b = JsChatBackend::new(vm, "local-shell");
        let turn = HostTurn {
            system: "role brief".into(),
            groups: vec![crate::chatbackend_provider::TurnGroup {
                entries: vec![ChatMsg {
                    id: "entry-1".into(),
                    kind: "user".into(),
                    content: "hello".into(),
                    ..ChatMsg::default()
                }],
            }],
        };

        b.send_turn("turn-1", turn.clone()).unwrap();
        let msgs = b.poll("turn-1", None).unwrap().messages;
        let forwarded: serde_json::Value = serde_json::from_str(&msgs[0].content).unwrap();

        assert_eq!(
            forwarded,
            json!({ "sid": "turn-1", "turn": serde_json::to_value(&turn).unwrap() }),
            "the adapter must hand the composed turn over unmodified"
        );
    }

    #[test]
    fn send_turn_missing_export_errors_unsupported() {
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        let err = b.send_turn("s1", HostTurn::default()).unwrap_err();

        assert!(err.to_string().contains(CONTEXT_OWNER_UNSUPPORTED));
    }

    #[test]
    fn context_owner_comes_from_manifest_key() {
        let manifest: PluginManifest = serde_json::from_str(
            r#"{
                "id": "context-owner-fixture",
                "version": "0.1.0",
                "displayName": "Context Owner Fixture",
                "capabilities": { "chatBackend": { "contextOwner": "host" } }
            }"#,
        )
        .unwrap();
        let b = JsChatBackend::from_manifest(vm_with("module.exports.default = {}"), &manifest);

        assert_eq!(b.context_owner(), ContextOwner::Host);
        assert_eq!(b.describe().context_owner, ContextOwner::Host);
    }

    #[test]
    fn context_owner_absent_manifest_key_is_plugin() {
        let manifest: PluginManifest = serde_json::from_str(
            r#"{
                "id": "context-owner-absent-fixture",
                "version": "0.1.0",
                "displayName": "Context Owner Absent Fixture",
                "capabilities": { "chatBackend": { "spawnable": true } }
            }"#,
        )
        .unwrap();
        let b = JsChatBackend::from_manifest(vm_with("module.exports.default = {}"), &manifest);

        assert_eq!(b.context_owner(), ContextOwner::Plugin);
    }

    #[test]
    fn watcher_capable_comes_from_manifest_flag() {
        let manifest: PluginManifest = serde_json::from_str(
            r#"{
                "id": "lmstudio",
                "version": "0.1.0",
                "displayName": "LM Studio",
                "capabilities": { "chatBackend": { "spawnable": true, "watcher": true } }
            }"#,
        )
        .unwrap();
        let b = JsChatBackend::from_manifest(vm_with("module.exports.default = {}"), &manifest);

        assert!(b.spawnable());
        assert!(b.watcher_capable());
    }

    #[test]
    fn pump_advances_state_that_poll_then_surfaces() {
        // pump() runs each tick before poll(); an async plugin advances work in pump, poll surfaces it.
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
        // switch_model routes to `setModel`; session_info reads the live model back via `sessionInfo`.
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
        // A bare backend with no `sessionInfo` export reports no model, never an error.
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.session_info("s").unwrap().model.is_none());
    }

    #[test]
    fn set_model_missing_export_errors() {
        // A backend that cannot switch surfaces an error (command reports "unsupported").
        let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
        let b = JsChatBackend::new(vm, "local-shell");
        assert!(b.set_model("s", "m").is_err());
    }
}
