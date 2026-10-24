use super::*;
use crate::chatbackend::{ChatBackendError, ChatBackendRegistry};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct FakeBackend {
    closed: AtomicBool,
    sends: AtomicUsize,
}

impl FakeBackend {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            closed: AtomicBool::new(false),
            sends: AtomicUsize::new(0),
        })
    }
}

impl ChatBackendProvider for FakeBackend {
    fn id(&self) -> &str {
        "liveness-fixture"
    }
    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("own-session".into())
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ChatBackendError::Call("channel closed".into()));
        }
        self.sends.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ChatBackendError::Call("channel closed".into()));
        }
        Ok(StructuredChat::default())
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn channel_closed_after_accept_is_detected_without_replaying() {
    let backend = FakeBackend::new();
    let bound = BoundSession::new("tab".into(), backend.clone());
    bound.preflight("own-session").unwrap();
    let waiting = bound.arm().unwrap();
    bound
        .provider
        .send_message("own-session", "opaque")
        .unwrap();
    backend.closed.store(true, Ordering::SeqCst);
    let error = bound.preflight("own-session").unwrap_err();
    assert!(error.contains("channel closed"));
    assert!(bound.fail(&error));
    assert!(!bound.fail(&error));
    assert_eq!(waiting.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    assert!(bound.arm().is_none());
    assert!(bound.current().is_err());
    assert_eq!(backend.sends.load(Ordering::SeqCst), 1);
}

#[test]
fn hot_reload_keeps_the_opening_adapter_and_never_sends_to_its_replacement() {
    let registry = ChatBackendRegistry::new();
    let old = FakeBackend::new();
    registry.register("liveness-fixture", old.clone());
    let bound = BoundSession::new("tab".into(), registry.get("liveness-fixture").unwrap());
    let replacement = FakeBackend::new();
    registry.register("liveness-fixture", replacement.clone());
    bound.preflight("own-session").unwrap();
    bound
        .provider
        .send_message("own-session", "opaque")
        .unwrap();
    assert_eq!(old.sends.load(Ordering::SeqCst), 1);
    assert_eq!(replacement.sends.load(Ordering::SeqCst), 0);
    old.closed.store(true, Ordering::SeqCst);
    assert!(bound.preflight("own-session").is_err());
    assert_eq!(replacement.sends.load(Ordering::SeqCst), 0);
    registry.unregister("liveness-fixture");
}

#[test]
fn turn_start_completes_the_failure_wait_once() {
    let bound = BoundSession::new("tab".into(), FakeBackend::new());
    let receiver = bound.arm().unwrap();
    assert!(bound.arm().is_none());
    bound.observe(&StructuredChat::default());
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
    bound.observe(&StructuredChat {
        done: Some(false),
        ..Default::default()
    });
    assert_eq!(receiver.try_recv(), Ok(()));
    assert!(bound.arm().is_some());
}

#[test]
fn fast_reply_proves_liveness_but_a_history_snapshot_does_not() {
    let bound = BoundSession::new("tab".into(), FakeBackend::new());
    bound.state.lock().assistant_ids.insert("history".into());
    let receiver = bound.arm().unwrap();
    let reply = |id: &str| StructuredChat {
        done: Some(true),
        messages: vec![crate::agent::chat_types::ChatMsg {
            id: id.into(),
            kind: "assistant".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    bound.observe(&reply("history"));
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
    bound.observe(&reply("new-reply"));
    assert_eq!(receiver.try_recv(), Ok(()));
}

fn service_fixture() -> (
    crate::service::test_fixture::ServiceFixture,
    String,
    Arc<FakeBackend>,
) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "liveness-fixture".into(),
        session_id: "own-session".into(),
    };
    tab.agent_type = Some("liveness-fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let backend = FakeBackend::new();
    fixture
        .service
        .chatbackend_registry
        .register("liveness-fixture", backend.clone());
    fixture
        .service
        .ext::<Arc<TabReopenGuards>>()
        .sessions
        .lock()
        .insert(
            ("liveness-fixture".into(), "own-session".into()),
            Arc::new(BoundSession::new(tab_id.clone(), backend.clone())),
        );
    (fixture, tab_id, backend)
}

#[test]
fn dead_backend_refusal_reaches_the_bubble_and_failed_recovery_raises_one_incident() {
    use crate::service::{
        chat_send_transaction::{run_chat_send, ChatSendState},
        delivery_error::DeliveryError,
        messenger::{AgentInput, HumanInput},
    };
    let (fixture, tab_id, backend) = service_fixture();
    let (sent, received) = mpsc::channel();
    let listener = fixture.ui.listen("notification-created", move |_| {
        let _ = sent.send(());
    });
    backend.closed.store(true, Ordering::SeqCst);
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&tab_id, "opaque", None)
        .unwrap();
    let result = run_chat_send(
        &fixture.service.chat_send_ledger,
        pending,
        "opaque",
        |text| {
            fixture
                .service
                .chatbackend_send(
                    "liveness-fixture",
                    "own-session",
                    &AgentInput::Human(HumanInput::composer(text)),
                )
                .map_err(|error| DeliveryError::from_opaque(&tab_id, error))
        },
    );
    assert_eq!(result.state, ChatSendState::Failed);
    assert!(result.error.unwrap().contains("chat_backend_unavailable"));
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    fixture.ui.unlisten(listener);
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed)
    );
    assert_eq!(
        tab.agent_state.unwrap().subtype,
        Some(codeterm_types::pty::QuestionSubtype::Error)
    );
    assert_eq!(backend.sends.load(Ordering::SeqCst), 0);
    let bound = fixture
        .service
        .chatbackend_bound_session("liveness-fixture", "own-session")
        .unwrap();
    fixture.service.chatbackend_liveness_failure(
        "liveness-fixture",
        "own-session",
        &bound,
        "closed again",
    );
    assert_eq!(
        fixture
            .service
            .notif
            .lock()
            .list(false)
            .iter()
            .filter(|notice| notice.title == "Chat backend unavailable")
            .count(),
        1
    );
}

#[test]
fn service_hot_reload_uses_bound_adapter_and_closed_after_accept_never_replays() {
    use crate::service::messenger::{AgentInput, HumanInput};
    let (fixture, _tab_id, backend) = service_fixture();
    let replacement = FakeBackend::new();
    fixture
        .service
        .chatbackend_registry
        .register("liveness-fixture", replacement.clone());
    fixture
        .service
        .chatbackend_send(
            "liveness-fixture",
            "own-session",
            &AgentInput::Human(HumanInput::composer("opaque")),
        )
        .unwrap();
    assert_eq!(backend.sends.load(Ordering::SeqCst), 1);
    assert_eq!(replacement.sends.load(Ordering::SeqCst), 0);
    backend.closed.store(true, Ordering::SeqCst);
    assert!(fixture
        .service
        .chatbackend_poll("liveness-fixture", "own-session", Some("own-cursor"))
        .is_err());
    assert!(fixture
        .service
        .chatbackend_send(
            "liveness-fixture",
            "own-session",
            &AgentInput::Human(HumanInput::composer("new"))
        )
        .is_err());
    assert_eq!(backend.sends.load(Ordering::SeqCst), 1);
    assert_eq!(replacement.sends.load(Ordering::SeqCst), 0);
}
