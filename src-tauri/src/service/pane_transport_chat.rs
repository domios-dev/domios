//! `ChatBackendTransport` — the non-PTY [`PaneTransport`] backed by a plugin
//! `chatBackend` capability (send → `sendMessage`, reads → polled transcript).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::pane_transport::PaneTransport;
use super::pty_io::SendBatchItem;
use super::ServiceContext;
use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::ModelInfo;
use crate::chatbackend::ChatBackendProvider;

/// Serializes the restore→reopen transition per pane so concurrent senders
/// can't each fire `openSession` and leave one pane with two plugin sessions.
#[derive(Default)]
pub struct PaneReopenGuards {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl PaneReopenGuards {
    fn lock_for(&self, pane_id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .entry(pane_id.to_string())
            .or_default()
            .clone()
    }

    /// Runs `transition` under the per-pane lock; the map lock is dropped first
    /// so different panes never block each other.
    pub fn serialized<T>(&self, pane_id: &str, transition: impl FnOnce() -> T) -> T {
        let lock = self.lock_for(pane_id);
        let _held = lock.lock();
        transition()
    }
}

/// Gate outbound preprocessor stages on agent-ness, mirroring the PTY
/// `is_user_turn_send` gate — a plain shell chatBackend owns its own prompt.
fn chatbackend_should_stage(pane_has_agent: bool) -> bool {
    pane_has_agent
}

/// `openSession` runs without the mux lock, so re-verify the pane is still the
/// same restored chatBackend before committing — else we'd attach to a dead pane.
fn chatbackend_reopen_still_valid(
    backing: Option<&crate::multiplexer::types::PaneBacking>,
    restored_from_snapshot: bool,
    expected_plugin: &str,
    expected_old_session: &str,
) -> bool {
    use crate::multiplexer::types::PaneBacking;
    restored_from_snapshot
        && matches!(
            backing,
            Some(PaneBacking::ChatBackend { plugin_id, session_id })
                if plugin_id == expected_plugin && session_id == expected_old_session
        )
}

fn build_chatbackend_open_session_ctx(
    pane_id: &str,
    workspace_id: Option<&str>,
    config: serde_json::Value,
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert(
        "paneId".into(),
        serde_json::Value::String(pane_id.to_string()),
    );
    if let Some(workspace_id) = workspace_id.filter(|id| !id.is_empty()) {
        ctx.insert(
            "workspaceId".into(),
            serde_json::Value::String(workspace_id.to_string()),
        );
    }
    for key in ["systemPrompt", "preset", "model"] {
        if let Some(value) = config.get(key).and_then(|v| v.as_str()) {
            ctx.insert(key.into(), serde_json::Value::String(value.to_string()));
        }
    }
    ctx.insert("config".into(), config);
    serde_json::Value::Object(ctx)
}

pub struct ChatBackendTransport<'a> {
    pub(crate) svc: &'a ServiceContext,
    pub(crate) pane_id: String,
    pub(crate) plugin_id: String,
    pub(crate) session_id: String,
}

impl PaneTransport for ChatBackendTransport<'_> {
    fn send(&self, text: &str, _enter: bool) -> Result<(), String> {
        // A pane restored after a daemon restart has no live plugin session; reopen lazily before the first send.
        let session_id = self
            .svc
            .chatbackend_reopen_if_restored(&self.pane_id)
            .unwrap_or(None)
            .unwrap_or_else(|| self.session_id.clone());
        let staged = if chatbackend_should_stage(self.svc.pane_has_agent(&self.pane_id)) {
            self.svc
                .run_outbound_stages(&self.pane_id, text.to_string())
        } else {
            text.to_string()
        };
        self.svc
            .chatbackend_send(&self.plugin_id, &session_id, &staged)
    }

    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String> {
        for item in batch {
            self.send(&item.text, item.enter)?;
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
    fn chatbackend_provider(
        &self,
        plugin_id: &str,
    ) -> Result<Arc<dyn ChatBackendProvider>, String> {
        self.chatbackend_registry
            .get(plugin_id)
            .ok_or_else(|| format!("chatBackend plugin '{plugin_id}' not registered"))
    }

    /// Open a plugin session for a pane; returns the plugin-chosen session id.
    pub fn chatbackend_open_session(
        &self,
        plugin_id: &str,
        pane_id: &str,
        config: serde_json::Value,
    ) -> Result<String, String> {
        let workspace_id = self.workspace_id_for_pane(pane_id);
        let ctx = build_chatbackend_open_session_ctx(pane_id, workspace_id.as_deref(), config);
        self.chatbackend_provider(plugin_id)?
            .open_session(ctx)
            .map_err(|e| e.to_string())
    }

    fn workspace_id_for_pane(&self, pane_id: &str) -> Option<String> {
        let snapshot = self.mux.read().snapshot();
        snapshot.workspaces.iter().find_map(|workspace| {
            workspace
                .tabs
                .iter()
                .any(|tab| tab.pane_id == pane_id)
                .then(|| workspace.id.clone())
        })
    }

    /// Lazily re-open a restored chatBackend pane's plugin session on first send:
    /// re-open from the persisted `_meta` config, migrate history to the new
    /// session id, swap the backing, start the poll loop. Returns the effective
    /// session id (or the live one), `None` for a non-chatBackend pane. Serialized
    /// per pane via [`PaneReopenGuards`] so only the first caller re-opens.
    pub fn chatbackend_reopen_if_restored(&self, pane_id: &str) -> Result<Option<String>, String> {
        use crate::multiplexer::types::PaneBacking;

        // The lock is the once-guard; the `restored_from_snapshot` recheck is the compare-and-swap.
        self.chatbackend_reopen_guards.serialized(pane_id, || {
            let (plugin_id, old_session) = {
                let mux = self.mux.write();
                let Some(resolved) = mux.resolve_pane_id(pane_id) else {
                    return Ok(None);
                };
                let Some(pane) = mux.get_pane(&resolved) else {
                    return Ok(None);
                };
                match (&pane.backing, pane.restored_from_snapshot) {
                    (
                        PaneBacking::ChatBackend {
                            plugin_id,
                            session_id,
                        },
                        true,
                    ) => (plugin_id.clone(), session_id.clone()),
                    // Already live (or a concurrent caller just reopened it): reuse the session id.
                    (PaneBacking::ChatBackend { session_id, .. }, false) => {
                        return Ok(Some(session_id.clone()));
                    }
                    _ => return Ok(None),
                }
            };

            let path = crate::service::pane_chat::chatbackend_history::session_path(
                &plugin_id,
                &old_session,
            );
            let config = crate::service::pane_chat::chatbackend_history::read_meta(&path)
                .unwrap_or(serde_json::Value::Null);

            let new_session = self.chatbackend_open_session(&plugin_id, pane_id, config.clone())?;

            // openSession ran without the mux lock; re-verify the pane before committing or we'd leak a session+loop onto a dead pane.
            let committed = {
                let mut mux = self.mux.write();
                let resolved = mux.resolve_pane_id(pane_id);
                match resolved.and_then(|r| mux.get_pane_mut(&r)) {
                    Some(pane)
                        if chatbackend_reopen_still_valid(
                            Some(&pane.backing),
                            pane.restored_from_snapshot,
                            &plugin_id,
                            &old_session,
                        ) =>
                    {
                        pane.backing = PaneBacking::ChatBackend {
                            plugin_id: plugin_id.clone(),
                            session_id: new_session.clone(),
                        };
                        pane.restored_from_snapshot = false;
                        true
                    }
                    _ => false,
                }
            };

            if !committed {
                // Pane vanished or re-backed during openSession: drop the orphan session.
                let _ = self.chatbackend_close(&plugin_id, &new_session);
                return Ok(None);
            }

            crate::service::pane_chat::chatbackend_history::migrate(
                &plugin_id,
                &old_session,
                &new_session,
            );

            self.spawn_chatbackend_poll_loop(
                pane_id.to_string(),
                plugin_id,
                new_session.clone(),
                config,
            );
            Ok(Some(new_session))
        })
    }

    /// Deliver one user turn to a plugin session.
    pub fn chatbackend_send(
        &self,
        plugin_id: &str,
        session_id: &str,
        text: &str,
    ) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .send_message(session_id, text)
            .map_err(|e| e.to_string())
    }

    /// Deliver one user turn to a chatBackend pane, re-opening a restored pane's
    /// dead session first so the raw send doesn't hit a session with no poll loop.
    pub(crate) fn chatbackend_pane_send(&self, pane_id: &str, text: &str) -> Result<(), String> {
        self.chatbackend_reopen_if_restored(pane_id)?;
        let (plugin_id, session_id) = self
            .pane_chat_backing(pane_id)
            .ok_or("chatBackend pane lost its backing")?;
        let staged = self.run_outbound_stages(pane_id, text.to_string());
        self.chatbackend_send(&plugin_id, &session_id, &staged)
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

    /// Tear down a plugin session (best-effort).
    pub fn chatbackend_close(&self, plugin_id: &str, session_id: &str) -> Result<(), String> {
        self.chatbackend_provider(plugin_id)?
            .close_session(session_id)
            .map_err(|e| e.to_string())
    }

    /// Models a chatBackend plugin offers, for the pane's model picker.
    pub fn chatbackend_list_models(&self, plugin_id: &str) -> Result<Vec<ModelInfo>, String> {
        self.chatbackend_provider(plugin_id)?
            .list_models()
            .map_err(|e| e.to_string())
    }

    pub fn chatbackend_list_presets(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<crate::chatbackend::PresetInfo>, String> {
        self.chatbackend_provider(plugin_id)?
            .list_presets()
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
    use std::sync::Arc;

    use parking_lot::Mutex;
    use serde_json::json;

    #[test]
    fn reopen_guard_serializes_to_a_single_open() {
        // N concurrent senders on a restored pane → only the first runs openSession.
        let guards = Arc::new(super::PaneReopenGuards::default());
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
                    guards.serialized("pane-1", || {
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
    fn reopen_guard_does_not_block_distinct_panes() {
        // Different panes take different locks, so neither serializes the other.
        let guards = super::PaneReopenGuards::default();
        let a = guards.serialized("pane-a", || "a");
        let b = guards.serialized("pane-b", || "b");
        assert_eq!((a, b), ("a", "b"));
    }

    #[test]
    fn reopen_recheck_rejects_a_pane_changed_during_open_session() {
        use crate::multiplexer::types::PaneBacking;
        let chat = |plugin: &str, sess: &str| PaneBacking::ChatBackend {
            plugin_id: plugin.into(),
            session_id: sess.into(),
        };

        // Pane closed mid-openSession.
        assert!(
            !super::chatbackend_reopen_still_valid(None, true, "lmstudio", "old"),
            "a vanished pane must not commit the migrate/poll-loop"
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
            "an already-committed pane must not re-open again"
        );
        assert!(!super::chatbackend_reopen_still_valid(
            Some(&PaneBacking::Pty),
            true,
            "lmstudio",
            "old",
        ));
        // Unchanged restored pane: the only case that commits.
        assert!(
            super::chatbackend_reopen_still_valid(
                Some(&chat("lmstudio", "old")),
                true,
                "lmstudio",
                "old",
            ),
            "an unchanged restored pane commits the reopen"
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

        assert_eq!(result, None, "a dead pane reopens to None");
        assert_eq!(
            closed.load(Ordering::SeqCst),
            1,
            "orphan session was closed"
        );
        assert_eq!(migrated.load(Ordering::SeqCst), 0, "no history migrate");
        assert_eq!(
            spawned.load(Ordering::SeqCst),
            0,
            "no poll loop spawned for a dead pane"
        );
    }

    #[test]
    fn chatbackend_send_is_gated_on_agent_pane() {
        assert!(
            !super::chatbackend_should_stage(false),
            "shell chatBackend send must skip outbound stages"
        );
        assert!(
            super::chatbackend_should_stage(true),
            "agent chatBackend send still runs outbound stages"
        );
    }

    #[test]
    fn chatbackend_open_session_ctx_promotes_open_shell_fields() {
        let ctx = super::build_chatbackend_open_session_ctx(
            "pane-7",
            Some("ws-1"),
            json!({
                "systemPrompt": "You are CodeTerm.",
                "preset": "codeterm",
                "model": "qwen",
                "temperature": 0.2
            }),
        );

        assert_eq!(ctx["paneId"], "pane-7");
        assert_eq!(ctx["workspaceId"], "ws-1");
        assert_eq!(ctx["config"]["temperature"], 0.2);
        assert_eq!(ctx["systemPrompt"], "You are CodeTerm.");
        assert_eq!(ctx["preset"], "codeterm");
        assert_eq!(ctx["model"], "qwen");
    }

    #[test]
    fn chatbackend_open_session_ctx_omits_absent_open_shell_fields() {
        let ctx = super::build_chatbackend_open_session_ctx("pane-7", None, json!({}));

        assert_eq!(ctx["paneId"], "pane-7");
        assert!(ctx.get("workspaceId").is_none());
        assert_eq!(ctx["config"], json!({}));
        assert!(ctx.get("systemPrompt").is_none());
        assert!(ctx.get("preset").is_none());
        assert!(ctx.get("model").is_none());
    }
}
