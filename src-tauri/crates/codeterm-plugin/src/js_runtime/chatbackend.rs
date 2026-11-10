//! `JsChatBackend` — adapts a JS `capabilities.chatBackend` plugin to the core
//! [`ChatBackendProvider`](crate::chatbackend_provider::ChatBackendProvider) trait via the
//! synchronous `call_json` (the VM is not `Send`, so it's called on the caller's thread).
//! Plugin surface: `openSession` / `sendMessage` / `watcherTick` / `poll` / `closeSession` / `listModels` / `listPresets`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm, VmError};
use super::PluginManifest;
use crate::chatbackend_provider::{
    ChatBackendError, ChatBackendProvider, ChatSessionInfo, HostTurn, PresetInfo,
    CONTEXT_OWNER_UNSUPPORTED,
};
use crate::provider::{ContextOwner, ModelCatalogueCapability, ModelInfo};
use codeterm_types::agent::chat_types::{ChatMsg, StructuredChat};

pub struct JsChatBackend {
    vm: Arc<PluginVm>,
    id: String,
    display_name: String,
    icon_svg: Option<String>,
    icon_color: Option<String>,
    /// `{ spawnable: true }` ⇒ user-openable from the picker; bare `chatBackend: true` ⇒ false (backend-only).
    spawnable: bool,
    watcher_capable: bool,
    session_config: bool,
    context_owner: ContextOwner,
    agent_tasks: bool,
    general_agent_offer_when_absent: bool,
    remote_task: Option<crate::remote_tasks::RemoteTaskCapability>,
    agent_runtime: Option<crate::agent_runtime::AgentRuntimeCapability>,
    model_catalogue: ModelCatalogueCapability,
    model_catalogue_input_files: Vec<PathBuf>,
    session_exec_env: parking_lot::Mutex<HashMap<String, HashMap<String, String>>>,
}

impl JsChatBackend {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            vm,
            display_name: id.clone(),
            id,
            icon_svg: None,
            icon_color: None,
            spawnable: false,
            watcher_capable: false,
            session_config: false,
            context_owner: ContextOwner::Plugin,
            agent_tasks: false,
            general_agent_offer_when_absent: false,
            remote_task: None,
            agent_runtime: None,
            model_catalogue: ModelCatalogueCapability::default(),
            model_catalogue_input_files: Vec::new(),
            session_exec_env: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    pub fn from_manifest(vm: Arc<PluginVm>, manifest: &PluginManifest) -> Self {
        Self {
            vm,
            id: manifest.id.clone(),
            display_name: manifest.display_name.clone(),
            icon_svg: manifest.icon_svg.clone(),
            icon_color: manifest.icon_color.clone(),
            spawnable: manifest.capabilities.chat_backend_spawnable(),
            watcher_capable: manifest.capabilities.chat_backend_watcher_capable(),
            session_config: manifest.capabilities.chat_backend.session_config(),
            context_owner: manifest.capabilities.chat_backend.context_owner(),
            agent_tasks: manifest.capabilities.chat_backend.agent_tasks
                && manifest.capabilities.remote_task.is_none(),
            general_agent_offer_when_absent: manifest.general_agent_offer_when_absent,
            remote_task: manifest.capabilities.remote_task.clone(),
            agent_runtime: manifest.capabilities.agent_runtime.clone(),
            model_catalogue: manifest.model_catalogue.clone(),
            model_catalogue_input_files: manifest.model_catalogue.fingerprint_files(
                super::host_credentials::resolved_files(&manifest.credentials),
            ),
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

    fn normalize_plugin_origins(&self, messages: &mut [ChatMsg]) {
        for message in messages {
            message.refresh_segments();
            if message.inbound_kind.as_deref() == Some("plugin")
                || message.from_plugin.is_some()
                || message.plugin_icon_svg.is_some()
            {
                message.inbound_kind = Some("plugin".into());
                message.from_plugin = Some(self.id.clone());
                message.plugin_icon_svg = self.icon_svg.clone();
            }
        }
    }
}

#[derive(Deserialize, ts_rs::TS)]
struct OpenOut {
    #[serde(default, rename = "sessionId", alias = "session_id")]
    session_id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// `poll` may return a bare `NormalizedChatMessage[]` (legacy/cursorless) or the
/// cursor-delta envelope `{ messages, cursor }`.
#[derive(Deserialize, ts_rs::TS)]
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

#[derive(Deserialize, ts_rs::TS)]
struct PluginModel {
    id: String,
    #[serde(default, rename = "displayName", alias = "display_name")]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    available: Option<bool>,
    #[serde(default)]
    group: Option<String>,
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
) -> Result<HashMap<String, String>, String> {
    let mut env = HashMap::from([("CODETERM_API_TOKEN".into(), String::new())]);
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
        if let (Some(tab_id), Some(nonce)) = (
            ctx.get("tabId").and_then(|value| value.as_str()),
            ctx.get("bindingNonce").and_then(|value| value.as_str()),
        ) {
            env.extend(svc.tab_process_env(tab_id, nonce)?);
        }
    }
    Ok(env)
}

impl ChatBackendProvider for JsChatBackend {
    fn general_agent_offer_when_absent(&self) -> bool {
        self.general_agent_offer_when_absent
    }

    fn agent_runtime(&self) -> Option<crate::agent_runtime::AgentRuntimeCapability> {
        self.agent_runtime.clone()
    }

    fn remote_task_capability(&self) -> Option<crate::remote_tasks::RemoteTaskCapability> {
        self.remote_task.clone()
    }

    fn discover_remote_task(
        &self,
    ) -> Result<Option<crate::remote_tasks::RemoteTaskDiscovery>, ChatBackendError> {
        if self.remote_task.is_none() {
            return Ok(None);
        }
        match call_json::<crate::remote_tasks::RemoteTaskDiscovery>(
            &self.vm,
            "discoverRemoteTask",
            &[],
        ) {
            Ok(discovery) => Ok(Some(discovery)),
            Err(VmError::MissingFunction(_)) => Ok(None),
            Err(VmError::Script(message)) if message.contains("not a function") => Ok(None),
            Err(error) => Err(map_err("discoverRemoteTask", error)),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supports_agent_tasks(&self) -> bool {
        self.agent_tasks
            && super::vm::has_method(&self.vm, "runAgentTask")
            && super::vm::has_method(&self.vm, "abortAgentTask")
    }

    fn run_agent_task(
        &self,
        request: &crate::agent_task::AgentTaskRequest,
    ) -> Result<crate::agent_task::AgentTaskResult, String> {
        crate::agent_task::validate_input(&request.input)?;
        if !self.supports_agent_tasks() {
            return Err("provider does not support bounded agent tasks".into());
        }
        if request.cancellation.is_cancelled() || std::time::Instant::now() >= request.deadline {
            return Err("agent task cancelled or expired".into());
        }
        let context = serde_json::to_value(&request.input).map_err(|e| e.to_string())?;
        let _scope =
            super::host_exec::host_env_scope(chatbackend_host_exec_env(&self.vm, &context)?);
        let output = super::vm::call_json_interruptible(
            &self.vm,
            "runAgentTask",
            &[context],
            request.cancellation.clone(),
            request.deadline,
        )
        .map_err(|e| format!("agent task failed: {e:?}"))?;
        crate::agent_task::validate_result(output)
    }

    fn abort_agent_task(&self, run_id: &str) -> Result<(), String> {
        if !self.supports_agent_tasks() {
            return Err("provider does not support bounded agent tasks".into());
        }
        super::vm::call_json_interruptible::<serde_json::Value>(
            &self.vm,
            "abortAgentTask",
            &[json!(run_id)],
            crate::converter::ConversionCancellation::default(),
            std::time::Instant::now() + std::time::Duration::from_secs(2),
        )
        .map(|_| ())
        .map_err(|e| format!("agent abort failed: {e:?}"))
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
        None
    }

    fn icon_svg(&self) -> Option<&str> {
        self.icon_svg.as_deref()
    }

    fn icon_color(&self) -> Option<&str> {
        self.icon_color.as_deref()
    }

    fn model_catalogue_capability(&self) -> ModelCatalogueCapability {
        self.model_catalogue.clone()
    }

    fn model_catalogue_input_files(&self) -> Vec<PathBuf> {
        self.model_catalogue_input_files.clone()
    }

    fn open_session(&self, ctx: serde_json::Value) -> Result<String, ChatBackendError> {
        let exec_env = chatbackend_host_exec_env(&self.vm, &ctx).map_err(ChatBackendError::Call)?;
        let _guard = super::host_exec::host_env_scope(exec_env.clone());
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
            PollOut::List(mut messages) => {
                self.normalize_plugin_origins(&mut messages);
                StructuredChat {
                    messages,
                    cursor: None,
                    // Legacy list pollers do not advertise busy state, so watcher arming is host-gated only.
                    done: None,
                    ..Default::default()
                }
            }
            PollOut::Wrapped {
                mut messages,
                cursor,
                done,
            } => {
                self.normalize_plugin_origins(&mut messages);
                StructuredChat {
                    messages,
                    cursor,
                    done,
                    ..Default::default()
                }
            }
        })
    }

    fn close_session(&self, session_id: &str) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        // Optional export; a missing `closeSession` is not an error.
        let _ = call_json::<serde_json::Value>(&self.vm, "closeSession", &[json!(session_id)]);
        self.session_exec_env.lock().remove(session_id);
        Ok(())
    }

    fn close_session_confirmed(
        &self,
        session_id: &str,
        deadline: std::time::Instant,
    ) -> Result<(), ChatBackendError> {
        let _guard = self.exec_env_scope(session_id);
        let receipt = super::vm::call_json_interruptible::<
            crate::chatbackend_provider::ChatBackendCloseReceipt,
        >(
            &self.vm,
            "closeSession",
            &[json!(session_id)],
            crate::converter::ConversionCancellation::default(),
            deadline,
        )
        .map_err(|error| map_err("closeSession", error))?;
        if receipt.session_id != session_id || !receipt.process_exited {
            return Err(ChatBackendError::Call(
                "closeSession did not confirm the owned session process exit".into(),
            ));
        }
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
                default_reasoning_effort: None,
                display_name: m.display_name.unwrap_or_else(|| m.id.clone()),
                id: m.id,
                description: m.description,
                available: m.available,
                availability: None,
                primary: None,
                featured: None,
                badge: None,
                group: m.group,
                reasoning_efforts: None,
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
mod tests;

pub(super) fn export_protocol_contracts(
    config: &ts_rs::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    use ts_rs::TS;
    PluginModel::export_all(config)?;
    OpenOut::export_all(config)?;
    PollOut::export_all(config)?;
    Ok(())
}
