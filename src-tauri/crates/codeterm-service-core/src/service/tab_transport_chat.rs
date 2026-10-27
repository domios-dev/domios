//! `ChatBackendTransport` — the non-PTY [`TabTransport`] backed by a plugin
//! `chatBackend` capability (send → `sendMessage`, reads → polled transcript).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::pty_io::SendBatchItem;
use super::tab_transport::TabTransport;
use super::ServiceContext;
use crate::agent::chat_types::StructuredChat;
use crate::chatbackend::ChatBackendProvider;
use crate::multiplexer::commands::MuxState;
mod liveness;
mod close;
mod recovery;
mod runtime_liveness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatBackendInstructionDelivery {
    SessionConfig,
    Blocked { code: &'static str },
}

pub fn removed_chatbackend_error(plugin_id: &str) -> String {
    serde_json::json!({
        "code": "chat_backend_plugin_removed",
        "plugin_id": plugin_id,
        "message": "Plugin removed. Open a new session with an installed provider."
    })
    .to_string()
}

fn has_system_prompt(config: &serde_json::Value) -> bool {
    config
        .get("systemPrompt")
        .and_then(|value| value.as_str())
        .is_some_and(|prompt| !prompt.trim().is_empty())
}

pub(crate) fn classify_chatbackend_instruction_delivery(
    descriptor: &crate::agent::provider::ProviderDescriptor,
    config: &serde_json::Value,
) -> ChatBackendInstructionDelivery {
    use crate::agent::provider::{ProviderTransport, SystemPromptDelivery};

    if !has_system_prompt(config) {
        return ChatBackendInstructionDelivery::SessionConfig;
    }
    match (&descriptor.transport, &descriptor.system_prompt_delivery) {
        (ProviderTransport::ChatBackend, SystemPromptDelivery::SessionConfig) => {
            ChatBackendInstructionDelivery::SessionConfig
        }
        (ProviderTransport::ChatBackend, _) => ChatBackendInstructionDelivery::Blocked {
            code: "chat_backend_session_config_required",
        },
        _ => ChatBackendInstructionDelivery::Blocked {
            code: "chat_backend_descriptor_transport_required",
        },
    }
}

/// A backend declaring host context ownership without the `sendTurn` entry
/// point is refused here — a silent fallback to `send_message` would reinstate
/// an unverifiable window undetectably.
fn open_chatbackend_session(
    provider: &dyn ChatBackendProvider,
    ctx: serde_json::Value,
) -> Result<String, String> {
    if !provider.context_owner().is_plugin() && !provider.supports_send_turn() {
        return Err(serde_json::json!({
            "code": "chat_backend_context_owner_unsupported",
            "delivery": "blocked",
            "transport": "chat_backend"
        })
        .to_string());
    }
    match classify_chatbackend_instruction_delivery(&provider.describe(), &ctx) {
        ChatBackendInstructionDelivery::SessionConfig => provider
            .open_session(ctx)
            .map_err(|error| error.to_string()),
        ChatBackendInstructionDelivery::Blocked { code } => Err(serde_json::json!({
            "code": code,
            "delivery": "blocked",
            "transport": "chat_backend",
            "required_capability": "chatBackend.sessionConfig",
            "message": "This plugin does not support system instructions. Spawning an agent requires a backend that accepts the launch system prompt through session configuration. Update the plugin or choose a compatible backend."
        })
        .to_string()),
    }
}

fn authenticate_chatbackend_tab(
    tab: &mut crate::multiplexer::types::Tab,
    plugin_id: &str,
    session_id: &str,
    nonce: &str,
) -> Result<(), String> {
    if !matches!(
        &tab.backing,
        crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: bound_plugin,
            session_id: bound_session,
        } if bound_plugin == plugin_id && bound_session == session_id
    ) {
        return Err("chatBackend provider session changed".into());
    }
    tab.agent_session_id = Some(session_id.to_string());
    tab.session_binding_nonce = Some(nonce.to_string());
    tab.reported_session_binding_nonce = Some(nonce.to_string());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    tab.chat_view = true;
    Ok(())
}

/// Serializes the restore→reopen transition per tab so concurrent senders
/// can't each fire `openSession` and leave one tab with two plugin sessions.
#[derive(Default)]
pub struct TabReopenGuards {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    sessions: Mutex<HashMap<(String, String), Arc<liveness::BoundSession>>>,
    recoveries: Mutex<HashMap<String, recovery::Recovery>>,
    turn_starts: Mutex<HashMap<String, runtime_liveness::EpochStarts>>,
}

impl TabReopenGuards {
    fn lock_for(&self, tab_id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .clone()
    }

    /// Runs `transition` under the per-tab lock; the map lock is dropped first
    /// so different tabs never block each other.
    pub fn serialized<T>(&self, tab_id: &str, transition: impl FnOnce() -> T) -> T {
        let lock = self.lock_for(tab_id);
        let _held = lock.lock();
        transition()
    }

    fn serialized_until<T>(
        &self,
        tab_id: &str,
        deadline: std::time::Instant,
        transition: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let lock = self.lock_for(tab_id);
        let _held = lock.try_lock_until(deadline).ok_or("chat backend close lock deadline")?;
        transition()
    }
}

/// `openSession` runs without the mux lock, so re-verify the tab is still the
/// same restored chatBackend before committing — else we'd attach to a dead tab.
fn chatbackend_reopen_still_valid(
    backing: Option<&crate::multiplexer::types::TabBacking>,
    restored_from_snapshot: bool,
    expected_plugin: &str,
    expected_old_session: &str,
) -> bool {
    use crate::multiplexer::types::TabBacking;
    restored_from_snapshot
        && matches!(
            backing,
            Some(TabBacking::ChatBackend { plugin_id, session_id })
                if plugin_id == expected_plugin && session_id == expected_old_session
        )
}

fn chatbackend_reopen_transition<Open, Close, After>(
    mux: &MuxState,
    guards: &TabReopenGuards,
    tab_id: &str,
    open: Open,
    close: Close,
    after_commit: After,
) -> Result<Option<String>, String>
where
    Open: Fn(&str, &str, &str) -> Result<(String, serde_json::Value), String>,
    Close: Fn(&str, &str),
    After: Fn(&str, &str, &str, &str, serde_json::Value) -> Result<(), String>,
{
    use crate::multiplexer::types::TabBacking;
    guards.serialized(tab_id, || {
        let (resolved, plugin_id, old_session) = {
            let mux = mux.read();
            let Some(resolved) = mux.resolve_tab_id(tab_id) else {
                return Ok(None);
            };
            let Some(tab) = mux.get_tab(&resolved) else {
                return Ok(None);
            };
            match (&tab.backing, tab.restored_from_snapshot) {
                (
                    TabBacking::ChatBackend {
                        plugin_id,
                        session_id,
                    },
                    true,
                ) => (resolved, plugin_id.clone(), session_id.clone()),
                (TabBacking::ChatBackend { session_id, .. }, false) => {
                    return Ok(Some(session_id.clone()));
                }
                _ => return Ok(None),
            }
        };
        let (new_session, config) = open(&plugin_id, &resolved, &old_session)?;
        let committed = {
            let mut mux = mux.write();
            match mux.get_tab_mut(&resolved) {
                Some(tab)
                    if chatbackend_reopen_still_valid(
                        Some(&tab.backing),
                        tab.restored_from_snapshot,
                        &plugin_id,
                        &old_session,
                    ) =>
                {
                    tab.backing = TabBacking::ChatBackend {
                        plugin_id: plugin_id.clone(),
                        session_id: new_session.clone(),
                    };
                    tab.restored_from_snapshot = false;
                    true
                }
                _ => false,
            }
        };
        if !committed {
            close(&plugin_id, &new_session);
            return Ok(None);
        }
        after_commit(&resolved, &plugin_id, &old_session, &new_session, config)?;
        Ok(Some(new_session))
    })
}

fn build_chatbackend_open_session_ctx(
    tab_id: &str,
    workspace_id: Option<&str>,
    pad_dir: Option<&str>,
    config: serde_json::Value,
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert(
        "tabId".into(),
        serde_json::Value::String(tab_id.to_string()),
    );
    if let Some(workspace_id) = workspace_id.filter(|id| !id.is_empty()) {
        ctx.insert(
            "workspaceId".into(),
            serde_json::Value::String(workspace_id.to_string()),
        );
    }
    if let Some(pad_dir) = pad_dir {
        ctx.insert(
            "padDir".into(),
            serde_json::Value::String(pad_dir.to_string()),
        );
    }
    for key in ["systemPrompt", "preset", "model", "mode"] {
        if let Some(value) = config.get(key).and_then(|v| v.as_str()) {
            ctx.insert(key.into(), serde_json::Value::String(value.to_string()));
        }
    }
    if let Some(value) = config.get("engine") {
        ctx.insert("engine".into(), value.clone());
    }
    ctx.insert("config".into(), config);
    serde_json::Value::Object(ctx)
}

/// The one place core asks who owns context, reached by both send entry points.
/// The question is a capability, never an identity — no provider id appears in
/// the expression, so none can leak into core.
pub(crate) fn dispatch_chatbackend_send(
    provider: &dyn ChatBackendProvider,
    plugin_id: &str,
    session_id: &str,
    input: &super::messenger::AgentInput,
) -> Result<(), String> {
    let rendered = input.text();
    let text = rendered.as_ref();
    use crate::agent::provider::ContextOwner;
    match provider.context_owner() {
        ContextOwner::Plugin => provider
            .send_message(session_id, text)
            .map_err(|e| e.to_string()),
        ContextOwner::Host => {
            crate::service::tab_chat::host_context::deliver_host_turn_for_session(
                provider, plugin_id, session_id, input,
            )
        }
    }
}

pub struct ChatBackendTransport<'a> {
    pub(crate) svc: &'a ServiceContext,
    pub(crate) tab_id: String,
    pub(crate) plugin_id: String,
    pub(crate) session_id: String,
}

impl TabTransport for ChatBackendTransport<'_> {
    fn send(&self, input: &super::messenger::AgentInput, _enter: bool) -> Result<(), String> {
        if super::agent_runtime::requested_for_tab(self.svc, &self.tab_id) {
            return super::agent_runtime::transport::send(self.svc, &self.tab_id, input, None)
                .map_err(|error| error.ledger_message());
        }
        // A tab restored after a daemon restart has no live plugin session; reopen lazily before the first send.
        let session_id = self
            .svc
            .chatbackend_reopen_if_restored(&self.tab_id)?
            .unwrap_or_else(|| self.session_id.clone());
        self.svc
            .chatbackend_send(&self.plugin_id, &session_id, input)
    }

    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String> {
        for item in batch {
            self.send(
                &super::messenger::AgentInput::Framed(
                    super::messenger::Messenger::from(super::messenger::MessageEnvelope::default())
                        .framed_delivery(&item.text),
                ),
                true,
            )?;
        }
        Ok(())
    }

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
        if super::agent_runtime::requested_for_tab(self.svc, &self.tab_id) {
            return super::agent_runtime::transport::interrupt(self.svc, &self.tab_id);
        }
        self.svc
            .chatbackend_close(&self.plugin_id, &self.session_id)
    }

    fn read_output(
        &self,
        _offset: Option<usize>,
        _max_bytes: Option<usize>,
        _strip_ansi: bool,
        _full: bool,
    ) -> Result<serde_json::Value, String> {
        let text = self.transcript_text()?;
        Ok(serde_json::json!({ "output": text, "offset": text.len() }))
    }

    fn read_screen(&self, formatted: bool) -> Result<serde_json::Value, String> {
        let text = self.transcript_text()?;
        Ok(serde_json::json!({ "text": text, "formatted": formatted }))
    }
}

impl ChatBackendTransport<'_> {
    fn transcript_text(&self) -> Result<String, String> {
        if super::agent_runtime::requested_for_tab(self.svc, &self.tab_id) {
            return super::agent_runtime::transport::snapshot(self.svc, &self.tab_id).map(
                |(_, messages)| {
                    messages
                        .into_iter()
                        .map(|message| message.content)
                        .collect::<Vec<_>>()
                        .join("\n")
                },
            );
        }
        // No cursor ⇒ the plugin returns the full transcript.
        let msgs = self
            .svc
            .chatbackend_poll(&self.plugin_id, &self.session_id, None)?
            .messages;
        let text = msgs
            .into_iter()
            .map(|m| m.content)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }
}

impl ServiceContext {
    pub(crate) fn bind_in_process_chatbackend_session(
        &self,
        tab_id: &str,
        plugin_id: &str,
        session_id: &str,
    ) -> Result<String, String> {
        let nonce = uuid::Uuid::new_v4().to_string();
        {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(tab_id)
                .ok_or_else(|| "chatBackend Tab disappeared".to_string())?;
            authenticate_chatbackend_tab(tab, plugin_id, session_id, &nonce)?;
        }
        self.set_and_lock_tab_agent_type_from(
            tab_id,
            plugin_id.to_string(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            None,
        );
        self.bind_planner_session(tab_id);
        Ok(nonce)
    }

    fn chatbackend_provider(
        &self,
        plugin_id: &str,
    ) -> Result<Arc<dyn ChatBackendProvider>, String> {
        self.chatbackend_registry
            .get(plugin_id)
            .ok_or_else(|| removed_chatbackend_error(plugin_id))
    }

    /// Open a plugin session for a tab; returns the plugin-chosen session id.
    pub fn chatbackend_open_session(
        &self,
        plugin_id: &str,
        tab_id: &str,
        config: serde_json::Value,
    ) -> Result<String, String> {
        let workspace_id = self.workspace_id_for_tab(tab_id);
        let pad = self.ensure_agent_pad(tab_id)?;
        let pad_dir = pad.env_value();
        let ctx = build_chatbackend_open_session_ctx(
            tab_id,
            workspace_id.as_deref(),
            Some(&pad_dir),
            config,
        );
        let provider = self.chatbackend_provider(plugin_id)?;
        let session_id = open_chatbackend_session(provider.as_ref(), ctx)?;
        self.ext::<Arc<TabReopenGuards>>().sessions.lock().insert(
            (plugin_id.into(), session_id.clone()),
            Arc::new(liveness::BoundSession::new(tab_id.into(), provider)),
        );
        Ok(session_id)
    }

    fn workspace_id_for_tab(&self, tab_id: &str) -> Option<String> {
        let snapshot = self.mux.read().snapshot();
        snapshot.workspaces.iter().find_map(|workspace| {
            workspace
                .tabs
                .contains(&tab_id.to_string())
                .then(|| workspace.id.clone())
        })
    }

    pub fn chatbackend_reopen_if_restored(&self, tab_id: &str) -> Result<Option<String>, String> {
        if super::agent_runtime::requested_for_tab(self, tab_id) {
            let session =
                crate::async_runtime::block_on(super::agent_runtime::engine::ensure(self, tab_id))
                    .map_err(|error| error.to_string())?;
            let thread = session.context.lock().thread_id.clone();
            return Ok(thread);
        }
        chatbackend_reopen_transition(
            &self.mux,
            self.ext::<Arc<crate::service::tab_transport_chat::TabReopenGuards>>(),
            tab_id,
            |plugin_id, tab_id, old_session| {
                let path = crate::service::tab_chat::chatbackend_history::session_path(
                    plugin_id,
                    old_session,
                );
                let mut config = crate::service::tab_chat::chatbackend_history::read_meta(&path)
                    .unwrap_or_else(|| serde_json::json!({}));
                let mode = self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.restorable_launch.execution_mode.clone());
                super::remote_execution::restore_config(&mut config, mode.as_deref())?;
                self.chatbackend_open_session(plugin_id, tab_id, config.clone())
                    .map(|session| (session, config))
            },
            |plugin_id, session_id| {
                let _ = self.chatbackend_close(plugin_id, session_id);
            },
            |tab_id, plugin_id, old_session, new_session, config| {
                self.bind_in_process_chatbackend_session(tab_id, plugin_id, new_session)?;
                crate::service::tab_chat::chatbackend_history::migrate(
                    plugin_id,
                    old_session,
                    new_session,
                );
                self.spawn_chatbackend_poll_loop(
                    tab_id.into(),
                    plugin_id.into(),
                    new_session.into(),
                    config,
                );
                self.schedule_planner_replay(tab_id);
                Ok(())
            },
        )
    }

    /// Deliver one user turn to a plugin session.
    pub fn chatbackend_send(
        &self,
        plugin_id: &str,
        session_id: &str,
        input: &super::messenger::AgentInput,
    ) -> Result<(), String> {
        if matches!(input, super::messenger::AgentInput::Shell(_)) {
            return Err("shell input cannot target a chat agent".into());
        }
        if self.chatbackend_session_is_watcher(plugin_id, session_id) {
            return Err(
                "this Tab is a watcher (read-only); its inputs come from the tick scheduler".into(),
            );
        }
        let bound = self.chatbackend_bound_session(plugin_id, session_id)?;
        self.chatbackend_send_bound(plugin_id, session_id, input, &bound)
    }

    fn chatbackend_send_bound(
        &self,
        plugin_id: &str,
        session_id: &str,
        input: &super::messenger::AgentInput,
        bound: &Arc<liveness::BoundSession>,
    ) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?;
        let provider = &bound.provider;
        if let Err(error) = bound.preflight(session_id) {
            self.chatbackend_liveness_failure(plugin_id, session_id, bound, &error);
            return Err(format!(
                "tab_busy_refused: chat_backend_unavailable: {error}"
            ));
        }
        self.arm_chatbackend_turn_deadline(bound);
        if let Err(error) =
            dispatch_chatbackend_send(provider.as_ref(), plugin_id, session_id, input)
        {
            self.chatbackend_liveness_failure(plugin_id, session_id, bound, &error);
            return Err(format!("submit_unconfirmed: {error}"));
        }
        Ok(())
    }

    fn chatbackend_session_is_watcher(&self, plugin_id: &str, session_id: &str) -> bool {
        use crate::multiplexer::types::TabBacking;

        self.mux.read().snapshot().tabs.iter().any(|(_, tab)| {
            tab.watcher.is_some()
                && matches!(
                    &tab.backing,
                    TabBacking::ChatBackend {
                        plugin_id: tab_plugin,
                        session_id: tab_session,
                    } if tab_plugin == plugin_id && tab_session == session_id
                )
        })
    }

    /// Deliver one user turn to a chatBackend tab, re-opening a restored tab's
    /// dead session first so the raw send doesn't hit a session with no poll loop.
    pub(crate) fn chatbackend_tab_send(
        &self,
        tab_id: &str,
        input: &super::messenger::AgentInput,
    ) -> Result<(), String> {
        if let Some((plugin_id, _)) = self.tab_chat_backing(tab_id) {
            if self.chatbackend_registry.get(&plugin_id).is_none()
                && self.agent_registry.provider_arc(&plugin_id).is_none()
            {
                return Err(removed_chatbackend_error(&plugin_id));
            }
        }
        self.wake_frozen_tab(tab_id)?;
        if super::agent_runtime::requested_for_tab(self, tab_id) {
            return super::agent_runtime::transport::send(self, tab_id, input, None)
                .map_err(|error| error.ledger_message());
        }
        self.chatbackend_reopen_if_restored(tab_id)?;
        let (plugin_id, session_id) = self
            .tab_chat_backing(tab_id)
            .ok_or("chatBackend Tab lost its backing")?;
        self.chatbackend_send(&plugin_id, &session_id, input)
    }

    pub(crate) fn captured_chat_identity(
        &self,
        tab_id: &str,
    ) -> Option<super::local_recording::chat::ChatIdentity> {
        super::local_recording::chat::ChatIdentity::from_tab(self.mux.read().get_tab(tab_id)?)
    }

    pub(crate) fn chatbackend_capture_send(
        &self,
        capture: &super::local_recording::chat::CapturedChat,
        input: &super::messenger::AgentInput,
    ) -> Result<(), super::delivery_error::DeliveryError> {
        self.require_managed_send_allowed(&capture.identity.tab_id, None)
            .map_err(
                |detail| super::delivery_error::DeliveryError::TabBusyRefused {
                    tab: capture.identity.tab_id.clone(),
                    detail,
                },
            )?;
        if self.chatbackend_session_is_watcher(
            &capture.identity.plugin_id,
            &capture.identity.session_id,
        ) {
            return Err(super::delivery_error::DeliveryError::TabBusyRefused {
                tab: capture.identity.tab_id.clone(),
                detail: "recording recipient is read-only".into(),
            });
        }
        let provider = self
            .chatbackend_bound_session(&capture.identity.plugin_id, &capture.identity.session_id)
            .map_err(
                |detail| super::delivery_error::DeliveryError::TabBusyRefused {
                    tab: capture.identity.tab_id.clone(),
                    detail,
                },
            )?;
        capture.handoff(
            self.captured_chat_identity(&capture.identity.tab_id),
            |plugin_id, session_id| {
                self.chatbackend_send_bound(plugin_id, session_id, input, &provider)
            },
        )
    }

    /// Advance an async chatBackend's in-flight work; called each poll tick, best-effort.
    pub fn chatbackend_pump(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_bound_session(plugin_id, session_id)?
            .provider
            .pump(session_id)
            .map_err(|e| e.to_string())
    }

    /// Pull the transcript delta after `cursor` (no cursor ⇒ full transcript).
    pub fn chatbackend_poll(
        &self,
        plugin_id: &str,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StructuredChat, String> {
        let bound = self.chatbackend_bound_session(plugin_id, session_id)?;
        self.chatbackend_provider(plugin_id)?;
        let result = bound.current().and_then(|()| {
            bound
                .provider
                .poll(session_id, cursor)
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(delta) => {
                bound.observe(&delta);
                Ok(delta)
            }
            Err(error) => {
                self.chatbackend_liveness_failure(plugin_id, session_id, &bound, &error);
                Err(error)
            }
        }
    }

    /// Enqueue one watcher machine tick; completion arrives through the poll loop.
    pub fn chatbackend_watcher_tick(
        &self,
        plugin_id: &str,
        session_id: &str,
        input: serde_json::Value,
    ) -> Result<(), String> {
        self.chatbackend_bound_session(plugin_id, session_id)?
            .provider
            .watcher_tick(session_id, input)
            .map_err(|e| e.to_string())
    }

    /// Tear down a plugin session (best-effort).
    pub fn chatbackend_close(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        let runtime_tabs: Vec<_> = self
            .mux
            .read()
            .snapshot()
            .tabs
            .values()
            .filter(|tab| {
                tab.restorable_launch.execution_mode.is_none()
                    && tab.agent_type.as_deref() == Some(plugin_id)
                    && tab.agent_session_id.as_deref() == Some(session_id)
            })
            .map(|tab| tab.id.clone())
            .collect();
        if !runtime_tabs.is_empty() && super::agent_runtime::engine::requested(self, plugin_id) {
            for tab in runtime_tabs {
                super::agent_runtime::engine::stop_blocking(self, &tab)?;
            }
            return Ok(());
        }
        let bound = self
            .ext::<Arc<TabReopenGuards>>()
            .sessions
            .lock()
            .remove(&(plugin_id.into(), session_id.into()));
        if let Some(bound) = bound {
            bound.cancel();
            bound
                .provider
                .close_session(session_id)
                .map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    /// Current live state of an open chatBackend session (the model it talks to).
    pub fn chatbackend_session_info(
        &self,
        plugin_id: &str,
        session_id: &str,
    ) -> Result<crate::chatbackend::ChatSessionInfo, String> {
        self.chatbackend_bound_session(plugin_id, session_id)?
            .provider
            .session_info(session_id)
            .map_err(|e| e.to_string())
    }

    /// Switch the model an open chatBackend session uses on its next turn, then
    /// return the refreshed session info so the caller can reflect it immediately.
    pub fn chatbackend_set_model(
        &self,
        plugin_id: &str,
        session_id: &str,
        model: &str,
    ) -> Result<crate::chatbackend::ChatSessionInfo, String> {
        let provider = self
            .chatbackend_bound_session(plugin_id, session_id)?
            .provider
            .clone();
        provider
            .set_model(session_id, model)
            .map_err(|e| e.to_string())?;
        provider.session_info(session_id).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
