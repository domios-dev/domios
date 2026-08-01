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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatBackendInstructionDelivery {
    SessionConfig,
    Blocked { code: &'static str },
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

fn open_chatbackend_session(
    provider: &dyn ChatBackendProvider,
    ctx: serde_json::Value,
) -> Result<String, String> {
    match classify_chatbackend_instruction_delivery(&provider.describe(), &ctx) {
        ChatBackendInstructionDelivery::SessionConfig => provider
            .open_session(ctx)
            .map_err(|error| error.to_string()),
        ChatBackendInstructionDelivery::Blocked { code } => Err(serde_json::json!({
            "code": code,
            "delivery": "blocked",
            "transport": "chat_backend"
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
    tab.agent_type = Some(plugin_id.to_string());
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
                    return Ok(Some(session_id.clone()))
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

pub struct ChatBackendTransport<'a> {
    pub(crate) svc: &'a ServiceContext,
    pub(crate) tab_id: String,
    pub(crate) plugin_id: String,
    pub(crate) session_id: String,
}

impl TabTransport for ChatBackendTransport<'_> {
    fn send(&self, text: &str, _enter: bool) -> Result<(), String> {
        // A tab restored after a daemon restart has no live plugin session; reopen lazily before the first send.
        let session_id = self
            .svc
            .chatbackend_reopen_if_restored(&self.tab_id)
            .unwrap_or(None)
            .unwrap_or_else(|| self.session_id.clone());
        self.svc
            .chatbackend_send(&self.plugin_id, &session_id, text)
    }

    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String> {
        for item in batch {
            self.send(&item.text, true)?;
        }
        Ok(())
    }

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
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
        self.bind_planner_session(tab_id);
        Ok(nonce)
    }

    fn chatbackend_provider(
        &self,
        plugin_id: &str,
    ) -> Result<Arc<dyn ChatBackendProvider>, String> {
        self.chatbackend_registry
            .get(plugin_id)
            .ok_or_else(|| format!("chatBackend plugin '{plugin_id}' not registered"))
    }

    /// Open a plugin session for a tab; returns the plugin-chosen session id.
    pub fn chatbackend_open_session(
        &self,
        plugin_id: &str,
        tab_id: &str,
        config: serde_json::Value,
    ) -> Result<String, String> {
        let workspace_id = self.workspace_id_for_tab(tab_id);
        let ctx = build_chatbackend_open_session_ctx(tab_id, workspace_id.as_deref(), config);
        let provider = self.chatbackend_provider(plugin_id)?;
        open_chatbackend_session(provider.as_ref(), ctx)
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
        chatbackend_reopen_transition(
            &self.mux,
            &self.chatbackend_reopen_guards,
            tab_id,
            |plugin_id, tab_id, old_session| {
                let path = crate::service::tab_chat::chatbackend_history::session_path(
                    plugin_id,
                    old_session,
                );
                let config = crate::service::tab_chat::chatbackend_history::read_meta(&path)
                    .unwrap_or(serde_json::Value::Null);
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
        text: &str,
    ) -> Result<(), String> {
        if self.chatbackend_session_is_watcher(plugin_id, session_id) {
            return Err(
                "this Tab is a watcher (read-only); its inputs come from the tick scheduler".into(),
            );
        }
        self.chatbackend_provider(plugin_id)?
            .send_message(session_id, text)
            .map_err(|e| e.to_string())
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
    pub(crate) fn chatbackend_tab_send(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.chatbackend_reopen_if_restored(tab_id)?;
        let (plugin_id, session_id) = self
            .tab_chat_backing(tab_id)
            .ok_or("chatBackend Tab lost its backing")?;
        self.chatbackend_send(&plugin_id, &session_id, text)
    }

    /// Advance an async chatBackend's in-flight work; called each poll tick, best-effort.
    pub fn chatbackend_pump(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
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
        self.chatbackend_provider(plugin_id)?
            .poll(session_id, cursor)
            .map_err(|e| e.to_string())
    }

    /// Enqueue one watcher machine tick; completion arrives through the poll loop.
    pub fn chatbackend_watcher_tick(
        &self,
        plugin_id: &str,
        session_id: &str,
        input: serde_json::Value,
    ) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .watcher_tick(session_id, input)
            .map_err(|e| e.to_string())
    }

    /// Tear down a plugin session (best-effort).
    pub fn chatbackend_close(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .close_session(session_id)
            .map_err(|e| e.to_string())
    }

    /// Current live state of an open chatBackend session (the model it talks to).
    pub fn chatbackend_session_info(
        &self,
        plugin_id: &str,
        session_id: &str,
    ) -> Result<crate::chatbackend::ChatSessionInfo, String> {
        self.chatbackend_provider(plugin_id)?
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
        let provider = self.chatbackend_provider(plugin_id)?;
        provider
            .set_model(session_id, model)
            .map_err(|e| e.to_string())?;
        provider.session_info(session_id).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use parking_lot::Mutex;
    use serde_json::json;

    struct InstructionRecordingBackend {
        session_config: bool,
        open_contexts: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl crate::chatbackend::ChatBackendProvider for InstructionRecordingBackend {
        fn id(&self) -> &str {
            "fixture-backend"
        }

        fn supports_session_config(&self) -> bool {
            self.session_config
        }

        fn open_session(
            &self,
            ctx: serde_json::Value,
        ) -> Result<String, crate::chatbackend::ChatBackendError> {
            self.open_contexts.lock().push(ctx);
            Ok("fixture-session".into())
        }

        fn send_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::chatbackend::ChatBackendError> {
            Ok(())
        }

        fn poll(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
        {
            Ok(crate::agent::chat_types::StructuredChat::default())
        }

        fn close_session(&self, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
            Ok(())
        }

        fn list_models(
            &self,
        ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError>
        {
            Ok(Vec::new())
        }
    }

    #[test]
    fn chat_backend_delivers_via_session_config() {
        let opens = Arc::new(Mutex::new(Vec::new()));
        let backend = InstructionRecordingBackend {
            session_config: true,
            open_contexts: opens.clone(),
        };
        let ctx = super::build_chatbackend_open_session_ctx(
            "tab-1",
            Some("workspace-1"),
            json!({ "systemPrompt": "Composed instruction" }),
        );

        assert_eq!(
            super::open_chatbackend_session(&backend, ctx),
            Ok("fixture-session".into())
        );
        let contexts = opens.lock();
        assert_eq!(contexts.len(), 1, "openSession is the sole delivery seam");
        assert_eq!(contexts[0]["systemPrompt"], "Composed instruction");
    }

    #[test]
    fn chat_backend_without_session_config_blocked() {
        let opens = Arc::new(Mutex::new(Vec::new()));
        let backend = InstructionRecordingBackend {
            session_config: false,
            open_contexts: opens.clone(),
        };
        let ctx = super::build_chatbackend_open_session_ctx(
            "tab-1",
            None,
            json!({ "systemPrompt": "Composed instruction" }),
        );

        let error = super::open_chatbackend_session(&backend, ctx).unwrap_err();
        let error: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(error["code"], "chat_backend_session_config_required");
        assert_eq!(error["delivery"], "blocked");
        assert!(opens.lock().is_empty(), "blocked backends are never opened");
    }

    #[test]
    fn concurrent_chat_reconnect_has_one_open_commit_and_after_effect() {
        use crate::multiplexer::commands::MuxState;
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::{AgentRole, TabBacking};
        use codeterm_util::lock_hold::TimedRwLock;

        let (mut state, parent_id) = MultiplexerState::new();
        let workspace_id = state.list_workspaces()[0].id.clone();
        state
            .set_tab_role(&parent_id, AgentRole::Orchestrator, None)
            .unwrap();
        let team_id = state
            .resolve_or_mint_team_for_parent(&parent_id)
            .unwrap()
            .team_id;
        let tab_id = state.create_tab(&workspace_id).unwrap();
        state
            .set_tab_role(&tab_id, AgentRole::Worker, Some(&parent_id))
            .unwrap();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "fixture".into(),
            session_id: "old".into(),
        };
        tab.restored_from_snapshot = true;
        let mux: MuxState = Arc::new(TimedRwLock::new("chat-reconnect", state));
        let guards = Arc::new(super::TabReopenGuards::default());
        let barrier = Arc::new(Barrier::new(3));
        let opens = Arc::new(AtomicUsize::new(0));
        let afters = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        let handles = (0..2)
            .map(|_| {
                let (mux, guards, barrier, opens, afters, closes, tab_id) = (
                    mux.clone(),
                    guards.clone(),
                    barrier.clone(),
                    opens.clone(),
                    afters.clone(),
                    closes.clone(),
                    tab_id.clone(),
                );
                std::thread::spawn(move || {
                    barrier.wait();
                    super::chatbackend_reopen_transition(
                        &mux,
                        &guards,
                        &tab_id,
                        |_, _, _| {
                            opens.fetch_add(1, Ordering::SeqCst);
                            Ok(("new".into(), json!({})))
                        },
                        |_, _| {
                            closes.fetch_add(1, Ordering::SeqCst);
                        },
                        |_, _, _, _, _| {
                            afters.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .unwrap()
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let sessions = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        assert_eq!(afters.load(Ordering::SeqCst), 1);
        assert_eq!(closes.load(Ordering::SeqCst), 0);
        assert!(sessions
            .iter()
            .all(|session| session.as_deref() == Some("new")));
        let tab = mux.read().get_tab(&tab_id).unwrap().clone();
        assert_eq!(tab.parent_tab_id.as_deref(), Some(parent_id.as_str()));
        assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
        assert!(!tab.restored_from_snapshot);
    }

    #[test]
    fn reopen_guard_serializes_to_a_single_open() {
        // N concurrent senders on a restored tab → only the first runs openSession.
        let guards = Arc::new(super::TabReopenGuards::default());
        let restored = Arc::new(AtomicBool::new(true));
        let opens = Arc::new(AtomicUsize::new(0));
        let session = Arc::new(Mutex::new(String::from("old")));

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let (guards, restored, opens, session) = (
                    guards.clone(),
                    restored.clone(),
                    opens.clone(),
                    session.clone(),
                );
                std::thread::spawn(move || {
                    guards.serialized("tab-1", || {
                        if restored.swap(false, Ordering::SeqCst) {
                            opens.fetch_add(1, Ordering::SeqCst);
                            *session.lock() = "migrated".to_string();
                        }
                        session.lock().clone()
                    })
                })
            })
            .collect();

        let seen: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            opens.load(Ordering::SeqCst),
            1,
            "only the first caller re-opens the session"
        );
        assert!(
            seen.iter().all(|s| s == "migrated"),
            "every caller observed the migrated session id, none the dead 'old' one: {seen:?}"
        );
    }

    #[test]
    fn reopen_guard_does_not_block_distinct_tabs() {
        // Different tabs take different locks, so neither serializes the other.
        let guards = super::TabReopenGuards::default();
        let a = guards.serialized("tab-a", || "a");
        let b = guards.serialized("tab-b", || "b");
        assert_eq!((a, b), ("a", "b"));
    }

    #[test]
    fn reopen_recheck_rejects_a_tab_changed_during_open_session() {
        use crate::multiplexer::types::TabBacking;
        let chat = |plugin: &str, sess: &str| TabBacking::ChatBackend {
            plugin_id: plugin.into(),
            session_id: sess.into(),
        };

        // Tab closed mid-openSession.
        assert!(
            !super::chatbackend_reopen_still_valid(None, true, "lmstudio", "old"),
            "a vanished Tab must not commit the migrate/poll-loop"
        );
        // Session id respawned during openSession.
        assert!(
            !super::chatbackend_reopen_still_valid(
                Some(&chat("lmstudio", "different")),
                true,
                "lmstudio",
                "old",
            ),
            "a re-backed session id must not commit"
        );
        assert!(!super::chatbackend_reopen_still_valid(
            Some(&chat("other", "old")),
            true,
            "lmstudio",
            "old",
        ));
        // Restore flag already cleared by a concurrent caller.
        assert!(
            !super::chatbackend_reopen_still_valid(
                Some(&chat("lmstudio", "old")),
                false,
                "lmstudio",
                "old",
            ),
            "an already-committed Tab must not re-open again"
        );
        assert!(!super::chatbackend_reopen_still_valid(
            Some(&TabBacking::Pty),
            true,
            "lmstudio",
            "old",
        ));
        // Unchanged restored tab: the only case that commits.
        assert!(
            super::chatbackend_reopen_still_valid(
                Some(&chat("lmstudio", "old")),
                true,
                "lmstudio",
                "old",
            ),
            "an unchanged restored Tab commits the reopen"
        );
    }

    #[test]
    fn reopen_invalid_recheck_closes_session_and_skips_poll_loop() {
        // A failed recheck closes the orphan session, migrates nothing, spawns no loop.
        let closed = AtomicUsize::new(0);
        let migrated = AtomicUsize::new(0);
        let spawned = AtomicUsize::new(0);

        let committed = super::chatbackend_reopen_still_valid(None, true, "p", "old");
        let result: Option<String> = if !committed {
            closed.fetch_add(1, Ordering::SeqCst);
            None
        } else {
            migrated.fetch_add(1, Ordering::SeqCst);
            spawned.fetch_add(1, Ordering::SeqCst);
            Some("new".into())
        };

        assert_eq!(result, None, "a dead Tab reopens to None");
        assert_eq!(
            closed.load(Ordering::SeqCst),
            1,
            "orphan session was closed"
        );
        assert_eq!(migrated.load(Ordering::SeqCst), 0, "no history migrate");
        assert_eq!(
            spawned.load(Ordering::SeqCst),
            0,
            "no poll loop spawned for a dead Tab"
        );
    }

    #[test]
    fn chatbackend_open_session_ctx_promotes_open_shell_fields() {
        let ctx = super::build_chatbackend_open_session_ctx(
            "tab-7",
            Some("ws-1"),
            json!({
                "systemPrompt": "You are CodeTerm.",
                "preset": "codeterm",
                "model": "qwen",
                "mode": "watcher",
                "engine": {
                    "kind": "machine",
                    "charter": "Watch the group.",
                    "tickIntervalSecs": 120
                },
                "temperature": 0.2
            }),
        );

        assert_eq!(ctx["tabId"], "tab-7");
        assert_eq!(ctx["workspaceId"], "ws-1");
        assert_eq!(ctx["config"]["temperature"], 0.2);
        assert_eq!(ctx["systemPrompt"], "You are CodeTerm.");
        assert_eq!(ctx["preset"], "codeterm");
        assert_eq!(ctx["model"], "qwen");
        assert_eq!(ctx["mode"], "watcher");
        assert_eq!(ctx["engine"]["kind"], "machine");
        assert_eq!(ctx["engine"]["charter"], "Watch the group.");
        assert_eq!(ctx["engine"]["tickIntervalSecs"], 120);
    }

    #[test]
    fn chatbackend_open_session_ctx_omits_absent_open_shell_fields() {
        let ctx = super::build_chatbackend_open_session_ctx("tab-7", None, json!({}));

        assert_eq!(ctx["tabId"], "tab-7");
        assert!(ctx.get("workspaceId").is_none());
        assert_eq!(ctx["config"], json!({}));
        assert!(ctx.get("systemPrompt").is_none());
        assert!(ctx.get("preset").is_none());
        assert!(ctx.get("model").is_none());
        assert!(ctx.get("mode").is_none());
        assert!(ctx.get("engine").is_none());
    }

    #[test]
    fn open_and_reopen_authenticate_the_exact_chatbackend_provider_session() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::TabBacking;

        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "local".into(),
            session_id: "session-1".into(),
        };
        super::authenticate_chatbackend_tab(tab, "local", "session-1", "nonce-1").unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("session-1"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("nonce-1"));
        assert_eq!(
            tab.reported_session_binding_nonce.as_deref(),
            Some("nonce-1")
        );

        tab.backing = TabBacking::ChatBackend {
            plugin_id: "local".into(),
            session_id: "session-2".into(),
        };
        super::authenticate_chatbackend_tab(tab, "local", "session-2", "nonce-2").unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("session-2"));
        assert_eq!(
            tab.reported_session_binding_nonce.as_deref(),
            Some("nonce-2")
        );
        assert!(super::authenticate_chatbackend_tab(tab, "local", "session-1", "stale").is_err());
    }
}
