use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::service::pty_io::SendBatchItem;
use codeterm_types::decision::{DecisionError, DecisionPrimitives};
use parking_lot::Mutex;

static NEXT_TAB: AtomicUsize = AtomicUsize::new(0);

fn tab() -> String {
    format!("tab-shadow-{}", NEXT_TAB.fetch_add(1, Ordering::Relaxed))
}

#[derive(Clone)]
struct StubHost {
    screen: String,
    reads: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
    models: Arc<SupporterRegistry>,
    token: bool,
    deliveries: Arc<Mutex<Vec<(ObservationTrigger, String, SignalAction)>>>,
    records: Arc<Mutex<Vec<String>>>,
}

impl StubHost {
    fn new(screen: &str, model: StubModel) -> (Self, Arc<AtomicUsize>, Arc<Mutex<Option<DecisionRequest>>>) {
        let calls = model.calls.clone();
        let seen = model.seen.clone();
        let registry = SupporterRegistry::new();
        registry.register("fixture", Arc::new(model));
        (
            Self {
                screen: screen.to_owned(),
                reads: Arc::default(),
                writes: Arc::default(),
                models: Arc::new(registry),
                token: true,
                deliveries: Arc::default(),
                records: Arc::default(),
            },
            calls,
            seen,
        )
    }
}

impl ShadowHost for StubHost {
    fn transport(&self, _tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        Ok(Box::new(StubTransport { host: self }))
    }

    fn supporter_models(&self) -> Arc<SupporterRegistry> {
        self.models.clone()
    }

    fn token_available(&self, _adapter_id: &str) -> bool {
        self.token
    }

    fn metadata(
        &self,
        _tab_id: &str,
        trigger: ObservationTrigger,
        structural_verdict: &str,
        screen: &str,
    ) -> Result<TabStateMetadata, String> {
        Ok(TabStateMetadata {
            context: serde_json::json!({
                "provider_capability": {"transport": "chat_backend", "display_name": "Fixture"},
                "requested_model": "requested",
                "session_model": "actual",
                "role": "Worker",
                "plan_id": "plan",
                "task_id": "task",
                "spawning_tab_id": "creator",
                "seconds_since_spawn": 2,
                "seconds_since_last_output": 1,
                "structural_verdict": structural_verdict,
                "pending_prompt_kind": "provider",
                "screen_changed": !screen.is_empty(),
                "observation_event": trigger.label(),
            }),
            session_key: "stub-session".into(),
            title: "Fixture worker".into(),
            spawning_tab_id: Some("creator".into()),
            parent_tab_id: Some("parent".into()),
            plan_id: Some("plan".into()),
            task_id: Some("task".into()),
            attempt_id: Some("attempt".into()),
            prompt_options: vec![NumberedPromptOption {
                number: 1,
                label: "Continue".into(),
                value: "1".into(),
            }],
        })
    }

    fn deliver(
        &self,
        _tab_id: &str,
        _metadata: &TabStateMetadata,
        _episode: u64,
        trigger: ObservationTrigger,
        label: &str,
        _confidence: f64,
        action: SignalAction,
    ) {
        self.deliveries
            .lock()
            .push((trigger, label.to_owned(), action));
    }

    fn record(&self, line: &str) {
        self.records.lock().push(line.to_owned());
    }
}

struct StubTransport<'a> {
    host: &'a StubHost,
}

impl StubTransport<'_> {
    fn write(&self) -> Result<(), String> {
        self.host.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl TabTransport for StubTransport<'_> {
    fn send(&self, _text: &str, _enter: bool) -> Result<(), String> {
        self.write()
    }

    fn send_batch(&self, _batch: Vec<SendBatchItem>) -> Result<(), String> {
        self.write()
    }

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        self.write()
    }

    fn kill(&self) -> Result<(), String> {
        self.write()
    }

    fn read_output(
        &self,
        _offset: Option<usize>,
        _max_bytes: Option<usize>,
        _strip_ansi: bool,
        _full: bool,
    ) -> Result<Value, String> {
        Err("observer must read the transport screen".into())
    }

    fn read_screen(&self, _formatted: bool) -> Result<Value, String> {
        self.host.reads.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({"text": self.host.screen}))
    }
}

struct StubModel {
    answer: Result<DecisionAnswer, DecisionError>,
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Option<DecisionRequest>>>,
}

impl StubModel {
    fn answering(answer: Result<DecisionAnswer, DecisionError>) -> Self {
        Self {
            answer,
            calls: Arc::default(),
            seen: Arc::default(),
        }
    }
}

impl DecisionModel for StubModel {
    fn id(&self) -> &str {
        "fixture"
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: false,
            score: false,
        }
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        _deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock() = Some(request.clone());
        self.answer.clone()
    }
}

fn choice(label: &str, confidence: f64) -> DecisionAnswer {
    DecisionAnswer::Choice {
        choice: label.to_owned(),
        probabilities: BTreeMap::from([(label.to_owned(), confidence)]),
        confidence: Some(confidence),
    }
}

fn enabled() -> SupporterTabStateConfig {
    SupporterTabStateConfig {
        enabled: true,
        ..SupporterTabStateConfig::default()
    }
}

fn run(host: &StubHost, tab_id: &str, trigger: ObservationTrigger) -> Option<ShadowVerdict> {
    spawn_observer(
        host,
        &enabled(),
        "fixture",
        3000,
        tab_id,
        trigger,
        "fixture-structural-verdict",
    )
    .expect("enabled observer starts")
    .join()
    .expect("observer thread completes")
}

#[test]
fn supporter_tab_state_disabled_makes_no_read_or_call() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let (host, calls, _) = StubHost::new("ready", model);
    assert!(spawn_observer(
        &host,
        &SupporterTabStateConfig::default(),
        "fixture",
        3000,
        &tab(),
        ObservationTrigger::SpawnReady,
        "ready",
    )
    .is_none());
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn tab_state_no_call_without_token() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let (mut host, calls, _) = StubHost::new("ready", model);
    host.token = false;
    assert!(run(&host, &tab(), ObservationTrigger::SpawnReady).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn tab_state_context_envelope_contains_all_runtime_fields() {
    let model = StubModel::answering(Ok(choice(WORKING_NORMALLY, 0.91)));
    let (host, calls, seen) = StubHost::new("transcript response", model);
    let tab_id = tab();
    run(&host, &tab_id, ObservationTrigger::WorkerIdle).expect("a valid verdict");

    let request = seen.lock().clone().expect("choice adapter received request");
    assert_eq!(request.state["screen"], "transcript response");
    for key in [
        "provider_capability",
        "requested_model",
        "session_model",
        "role",
        "plan_id",
        "task_id",
        "spawning_tab_id",
        "seconds_since_spawn",
        "seconds_since_last_output",
        "structural_verdict",
        "pending_prompt_kind",
        "screen_changed",
    ] {
        assert!(request.state["context"].get(key).is_some(), "missing {key}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.reads.load(Ordering::SeqCst), 1);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn tab_state_mock_transport_uses_chat_backend_transcript() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.95)));
    let (host, _, seen) = StubHost::new("assistant: transcript", model);
    run(&host, &tab(), ObservationTrigger::SpawnReady).expect("transcript was classified");
    let request = seen.lock().clone().expect("choice adapter received request");
    assert_eq!(request.state["screen"], "assistant: transcript");
    assert_eq!(
        request.state["context"]["provider_capability"]["transport"],
        "chat_backend"
    );
}

#[test]
fn tab_state_healthy_spawn_is_spawn_ok_without_action() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.95)));
    let (host, calls, _) = StubHost::new("ready composer", model);
    let verdict = run(&host, &tab(), ObservationTrigger::SpawnReady)
        .expect("healthy spawn was classified");

    assert_eq!(verdict.action, SignalAction::None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.deliveries.lock().as_slice(),
        &[(
            ObservationTrigger::SpawnReady,
            READY_IDLE.to_owned(),
            SignalAction::SpawnOk,
        )]
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn tab_state_strong_actions_require_two_observations_and_never_write() {
    let model = StubModel::answering(Ok(choice(ERROR_LOOP, 0.94)));
    let (host, calls, _) = StubHost::new("repeating error", model);
    let tab_id = tab();
    assert_eq!(
        run(&host, &tab_id, ObservationTrigger::WorkerIdle)
            .expect("first verdict")
            .action,
        SignalAction::WeakHint
    );
    assert_eq!(
        run(&host, &tab_id, ObservationTrigger::TabExit)
            .expect("second verdict")
            .action,
        SignalAction::StrongAction
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn tab_state_strong_trust_keeps_pending_options_for_orchestrator() {
    let model = StubModel::answering(Ok(choice(TRUST_OR_PERMISSION_PROMPT, 0.93)));
    let (host, _, _) = StubHost::new("permission question", model);
    let tab_id = tab();
    run(&host, &tab_id, ObservationTrigger::WorkerIdle);
    run(&host, &tab_id, ObservationTrigger::TabExit);
    assert!(host.deliveries.lock().iter().any(|(_, label, action)| {
        label == TRUST_OR_PERMISSION_PROMPT && *action == SignalAction::StrongAction
    }));
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn tab_state_still_starting_is_suppressed_before_deadline() {
    let model = StubModel::answering(Ok(choice(STILL_STARTING, 0.93)));
    let (host, _, _) = StubHost::new("boot output", model);
    let tab_id = tab();
    run(&host, &tab_id, ObservationTrigger::SpawnReady).expect("valid classification");
    assert!(host.deliveries.lock().is_empty());
    assert!(host.records.lock().iter().any(|line| line.contains("suppressed")));
}

#[test]
fn tab_state_uses_configured_adapter_with_choice_capability() {
    let registry = SupporterRegistry::new();
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let calls = model.calls.clone();
    registry.register("other", Arc::new(model));
    assert!(choice_model(&registry, "fixture").is_none());
    assert!(choice_model(&registry, "other").is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn tab_state_context_header_fields_present() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = format!("tab-state-context-{}", uuid::Uuid::new_v4().simple());
    mux.link_pty(&tab_id, pty_id.clone());
    {
        let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
        tab.agent_type = Some("tab-state-provider".into());
        tab.agent_set_at = Some(Instant::now());
        tab.agent_requested_model = Some("requested-model".into());
        tab.agent_session_model = Some("session-model".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.role = Some(codeterm_types::agent::AgentRole::Worker);
        tab.spawned_by_tab_id = Some("creator-tab".into());
        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some("plan-id".into()),
            task_id: Some("task-id".into()),
            attempt_id: Some("attempt-id".into()),
            spawn_handle: None,
            bound_at: 1,
        });
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("tab-state-provider"),
    ));
    fixture
        .service
        .screen_state
        .lock()
        .process(&pty_id, b"opaque terminal screen");
    let screen = fixture
        .service
        .tab_transport(&tab_id)
        .expect("PTY transport")
        .read_screen(false)
        .expect("PTY screen")["text"]
        .as_str()
        .expect("screen text")
        .to_owned();
    let metadata = <ServiceContext as ShadowHost>::metadata(
        &fixture.service,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "ready-verdict",
        &screen,
    )
    .expect("runtime context");

    assert_eq!(metadata.context["provider_capability"]["transport"], "pty");
    assert_eq!(metadata.context["requested_model"], "requested-model");
    assert_eq!(metadata.context["session_model"], "session-model");
    assert_eq!(metadata.context["role"], "Worker");
    assert_eq!(metadata.context["plan_id"], "plan-id");
    assert_eq!(metadata.context["task_id"], "task-id");
    assert_eq!(metadata.context["task_binding"]["attempt_id"], "attempt-id");
    assert_eq!(metadata.context["spawning_tab_id"], "creator-tab");
    assert!(metadata.context["provider_capability"]["display_name"].is_string());
    for key in [
        "seconds_since_spawn",
        "seconds_since_last_output",
        "structural_verdict",
        "pending_prompt_kind",
        "screen_changed",
    ] {
        assert!(metadata.context.get(key).is_some(), "missing {key}");
    }
}

struct TranscriptBackend;

impl crate::chatbackend::ChatBackendProvider for TranscriptBackend {
    fn id(&self) -> &str {
        "tab-state-chat-fixture"
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn open_session(&self, _config: Value) -> Result<String, crate::chatbackend::ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(
        &self,
        _session_id: &str,
        _text: &str,
    ) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError> {
        Ok(crate::agent::chat_types::StructuredChat {
            messages: vec![crate::agent::chat_types::ChatMsg {
                id: "assistant-message".into(),
                kind: "assistant".into(),
                content: "assistant transcript evidence".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn close_session(&self, _session_id: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn tab_state_chat_backend_reads_transcript_by_capability() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
        tab.agent_type = Some("tab-state-chat-fixture".into());
        tab.agent_session_id = Some("session".into());
        tab.session_binding_nonce = Some("chat-session-nonce".into());
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "tab-state-chat-fixture".into(),
            session_id: "session".into(),
        };
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .chatbackend_registry
        .register("tab-state-chat-fixture", Arc::new(TranscriptBackend));
    let screen = fixture
        .service
        .tab_transport(&tab_id)
        .expect("chat backend transport")
        .read_screen(false)
        .expect("transcript read")["text"]
        .as_str()
        .expect("transcript text")
        .to_owned();
    let metadata = <ServiceContext as ShadowHost>::metadata(
        &fixture.service,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "ready-verdict",
        &screen,
    )
    .expect("runtime context");

    assert!(screen.contains("assistant transcript evidence"));
    assert_eq!(
        metadata.context["provider_capability"]["transport"],
        "chat_backend"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn tab_state_never_answers_prompt() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).expect("fixture tab").pending_prompt =
        Some(codeterm_types::pty::PendingPrompt {
            question: "opaque prompt".into(),
            options: vec![codeterm_types::pty::PromptOption {
                label: "Opaque option".into(),
                value: "1".into(),
            }],
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    delivery::deliver(
        &fixture.service,
        &tab_id,
        &TabStateMetadata {
            context: Value::Null,
            session_key: "fixture-session".into(),
            title: "worker".into(),
            spawning_tab_id: None,
            parent_tab_id: None,
            plan_id: None,
            task_id: None,
            attempt_id: None,
            prompt_options: Vec::new(),
        },
        0,
        ObservationTrigger::BusyNoOutput,
        TRUST_OR_PERMISSION_PROMPT,
        0.92,
        SignalAction::StrongAction,
    );
    let mux = fixture.service.mux.read();
    assert!(mux
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.pending_prompt.is_some()));
}
