//! The discriminator pair: same transcript, same user text, same fixture
//! backend shape — the declared context owner is the ONLY difference, so a
//! difference in the path taken can only come from the capability.
//!
//! What is proven here bounds the HOST TURN, not each HTTP request: a plugin
//! tool loop re-enters per round-trip and only the first call is host-composed.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;

use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::{ContextOwner, ModelInfo};
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, HostTurn};

struct ContextOwnerRecordingBackend {
    owner: ContextOwner,
    exports_send_turn: bool,
    opens: Arc<AtomicUsize>,
    messages: Arc<Mutex<Vec<(String, String)>>>,
    turns: Arc<Mutex<Vec<(String, HostTurn)>>>,
}

impl ContextOwnerRecordingBackend {
    fn new(owner: ContextOwner) -> Self {
        Self {
            owner,
            exports_send_turn: true,
            opens: Arc::new(AtomicUsize::new(0)),
            messages: Arc::new(Mutex::new(Vec::new())),
            turns: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ChatBackendProvider for ContextOwnerRecordingBackend {
    fn id(&self) -> &str {
        "context-owner-discriminator-fixture"
    }

    fn supports_session_config(&self) -> bool {
        true
    }

    fn context_owner(&self) -> ContextOwner {
        self.owner
    }

    fn supports_send_turn(&self) -> bool {
        self.exports_send_turn
    }

    fn send_turn(&self, session_id: &str, turn: HostTurn) -> Result<(), ChatBackendError> {
        self.turns.lock().push((session_id.to_string(), turn));
        Ok(())
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok("fixture-session".into())
    }

    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError> {
        self.messages
            .lock()
            .push((session_id.to_string(), text.to_string()));
        Ok(())
    }

    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        Ok(StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

/// A synthetic session whose host-side files are removed either side of the
/// send, so each case asserts only on files it created itself.
struct DiscriminatorSession {
    plugin: &'static str,
    session: &'static str,
}

impl DiscriminatorSession {
    fn new(plugin: &'static str) -> Self {
        let fixture = Self {
            plugin,
            session: "discriminator-session",
        };
        fixture.clear();
        fixture
    }

    fn sidecar(&self) -> std::path::PathBuf {
        crate::service::tab_chat::chatbackend_history::context_sidecar_path(
            self.plugin,
            self.session,
        )
    }

    fn clear(&self) {
        let _ = std::fs::remove_file(self.sidecar());
        let _ = std::fs::remove_file(crate::service::tab_chat::chatbackend_history::session_path(
            self.plugin,
            self.session,
        ));
    }
}

impl Drop for DiscriminatorSession {
    fn drop(&mut self) {
        self.clear();
    }
}

#[test]
fn plugin_context_owner_send_path_is_unchanged() {
    let fixture = DiscriminatorSession::new("context-owner-plugin-discriminator");
    let backend = ContextOwnerRecordingBackend::new(ContextOwner::Plugin);

    super::super::dispatch_chatbackend_send(
        &backend,
        fixture.plugin,
        fixture.session,
        "the identical ask",
    )
    .unwrap();

    assert_eq!(
        backend.messages.lock().as_slice(),
        &[(fixture.session.to_string(), "the identical ask".to_string())],
        "send_message received exactly (session_id, text)"
    );
    assert!(
        backend.turns.lock().is_empty(),
        "send_turn is never called without the capability"
    );
    assert!(
        !fixture.sidecar().exists(),
        "the unchanged path records no context decision"
    );
}

#[test]
fn host_context_owner_uses_send_turn_and_never_send_message() {
    let fixture = DiscriminatorSession::new("context-owner-host-discriminator");
    let backend = ContextOwnerRecordingBackend::new(ContextOwner::Host);

    super::super::dispatch_chatbackend_send(
        &backend,
        fixture.plugin,
        fixture.session,
        "the identical ask",
    )
    .unwrap();

    let turns = backend.turns.lock();
    assert_eq!(turns.len(), 1, "send_turn took the identical input once");
    assert_eq!(turns[0].0, fixture.session);
    assert_eq!(
        turns[0].1.groups.last().unwrap().entries[0].content,
        "the identical ask",
        "the user turn's content crosses the boundary verbatim"
    );
    assert!(
        backend.messages.lock().is_empty(),
        "send_message is never called with the capability"
    );
    assert!(
        fixture.sidecar().exists(),
        "the host-composed turn left a decision record"
    );
}

#[test]
fn host_context_owner_without_send_turn_export_is_refused_at_open() {
    let mut backend = ContextOwnerRecordingBackend::new(ContextOwner::Host);
    backend.exports_send_turn = false;
    let ctx = super::super::build_chatbackend_open_session_ctx(
        "tab-1",
        None,
        None,
        json!({ "systemPrompt": "Composed instruction" }),
    );

    let error = super::super::open_chatbackend_session(&backend, ctx).unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "chat_backend_context_owner_unsupported");
    assert_eq!(error["delivery"], "blocked");
    assert_eq!(
        backend.opens.load(Ordering::SeqCst),
        0,
        "a refused declaration produces no session id and never opens"
    );

    backend.exports_send_turn = true;
    let ctx = super::super::build_chatbackend_open_session_ctx(
        "tab-1",
        None,
        None,
        json!({ "systemPrompt": "Composed instruction" }),
    );
    assert_eq!(
        super::super::open_chatbackend_session(&backend, ctx),
        Ok("fixture-session".into()),
        "declaring host ownership WITH the export opens normally"
    );
}
