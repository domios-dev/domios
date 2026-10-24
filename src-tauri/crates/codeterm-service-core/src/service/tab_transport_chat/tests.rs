mod context_owner;

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

    fn send_message(&self, _: &str, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
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
    ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError> {
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
        None,
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
        None,
        json!({ "systemPrompt": "Composed instruction" }),
    );

    let error = super::open_chatbackend_session(&backend, ctx).unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "chat_backend_session_config_required");
    assert_eq!(error["delivery"], "blocked");
    assert_eq!(error["required_capability"], "chatBackend.sessionConfig");
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
    state.set_orchestrator_role(&parent_id).unwrap();
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
fn restored_missing_plugin_keeps_identity_and_session_without_cleanup() {
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TabBacking;
    use codeterm_util::lock_hold::TimedRwLock;

    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "removed-fixture".into(),
        session_id: "preserved-session".into(),
    };
    tab.restored_from_snapshot = true;
    let before = tab.clone();
    let mux: MuxState = Arc::new(TimedRwLock::new("removed-plugin", state));
    let result = super::chatbackend_reopen_transition(
        &mux,
        &super::TabReopenGuards::default(),
        &tab_id,
        |plugin, _, _| Err(super::removed_chatbackend_error(plugin)),
        |_, _| panic!("a missing plugin created no session to close"),
        |_, _, _, _, _| panic!("a missing plugin cannot commit a reopened session"),
    );
    let error: serde_json::Value = serde_json::from_str(&result.unwrap_err()).unwrap();
    assert_eq!(error["code"], "chat_backend_plugin_removed");
    assert_eq!(error["plugin_id"], "removed-fixture");
    let after = mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(after.id, before.id);
    assert_eq!(after.backing, before.backing);
    assert_eq!(after.agent_session_id, before.agent_session_id);
    assert_eq!(after.parent_tab_id, before.parent_tab_id);
    assert_eq!(after.team_id, before.team_id);
    assert!(after.restored_from_snapshot);
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
        Some("/instance/runtime/agent-pads/tab-7"),
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
    assert_eq!(ctx["padDir"], "/instance/runtime/agent-pads/tab-7");
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
    let ctx = super::build_chatbackend_open_session_ctx("tab-7", None, None, json!({}));

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
