use super::*;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::service::pty_io::SendBatchItem;
use codeterm_decision::{
    DecisionError, DecisionPrimitives, DecisionRequest, DecisionResponse, TokenMode,
};
use parking_lot::Mutex;

static NEXT_TAB: AtomicUsize = AtomicUsize::new(0);

fn tab() -> String {
    format!("tab-shadow-{}", NEXT_TAB.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug)]
struct EpisodeStep {
    name: String,
    state: state::EpisodeSnapshot,
}

fn push_episode_step(steps: &Mutex<Vec<EpisodeStep>>, name: impl Into<String>, tab_id: &str) {
    let state = state::snapshot_episode(tab_id);
    steps.lock().push(EpisodeStep {
        name: name.into(),
        state,
    });
}

fn episode_steps_table(steps: &Mutex<Vec<EpisodeStep>>) -> String {
    let mut table = String::from(
        "step | session_key | episode | output_sequence | signaled | spawn_observed\n",
    );
    for step in steps.lock().iter() {
        table.push_str(&format!(
            "{} | {} | {} | {} | {} | {}\n",
            step.name,
            step.state.session_key,
            step.state.episode,
            step.state.output_sequence,
            step.state.signaled,
            step.state.spawn_observed,
        ));
    }
    table
}

fn screen_hash(screen: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    screen.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone)]
struct StubHost {
    screen: String,
    reads: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
    models: Arc<SupporterRegistry>,
    token: bool,
    metadata_service: Option<ServiceContext>,
    session_key: Option<String>,
    episode_steps: Option<Arc<Mutex<Vec<EpisodeStep>>>>,
    deliveries: Arc<Mutex<Vec<(ObservationTrigger, String, SignalAction)>>>,
    records: Arc<Mutex<Vec<String>>>,
    runtime: Arc<SupporterRuntime>,
    _journal_dir: Arc<tempfile::TempDir>,
}

impl StubHost {
    fn new(
        screen: &str,
        model: StubModel,
    ) -> (
        Self,
        Arc<AtomicUsize>,
        Arc<Mutex<Option<DecisionQuestionsRequest>>>,
    ) {
        let calls = model.calls.clone();
        let seen = model.seen.clone();
        let registry = SupporterRegistry::new();
        registry.register("fixture", Arc::new(model));
        let journal_dir = Arc::new(tempfile::tempdir().unwrap());
        (
            Self {
                screen: screen.to_owned(),
                reads: Arc::default(),
                writes: Arc::default(),
                models: Arc::new(registry),
                token: true,
                metadata_service: None,
                session_key: None,
                episode_steps: None,
                deliveries: Arc::default(),
                records: Arc::default(),
                runtime: Arc::new(SupporterRuntime::new(journal_dir.path().into())),
                _journal_dir: journal_dir,
            },
            calls,
            seen,
        )
    }
}

impl ShadowHost for StubHost {
    fn local_identity(&self, tab: &str) -> Option<identity::LocalIdentity> {
        self.metadata_service
            .as_ref()
            .and_then(|service| service.local_identity(tab))
    }
    fn ledger_snapshot(&self, tab: &str) -> coordinator::LedgerSnapshot {
        self.metadata_service
            .as_ref()
            .map_or_else(Default::default, |service| service.ledger_snapshot(tab))
    }
    fn observation_is_current(&self, tab: &str, metadata: &TabStateMetadata) -> bool {
        self.metadata_service.as_ref().is_none_or(|service| {
            metadata
                .envelope
                .as_ref()
                .and_then(|envelope| envelope.local.as_ref())
                .is_some_and(|expected| service.observation_identity_is_current(tab, expected))
        })
    }
    fn transport(&self, _tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        Ok(Box::new(StubTransport { host: self }))
    }

    fn supporter_models(&self) -> Arc<SupporterRegistry> {
        self.models.clone()
    }

    fn supporter_runtime(&self) -> Arc<SupporterRuntime> {
        self.runtime.clone()
    }

    fn token_available(&self, _adapter_id: &str) -> bool {
        self.token
    }

    fn metadata(
        &self,
        tab_id: &str,
        trigger: ObservationTrigger,
        structural_verdict: &str,
        screen: &str,
    ) -> Result<TabStateMetadata, String> {
        if let Some(service) = &self.metadata_service {
            return <ServiceContext as ShadowHost>::metadata(
                service,
                tab_id,
                trigger,
                structural_verdict,
                screen,
            );
        }
        let session_key = self
            .session_key
            .clone()
            .unwrap_or_else(|| "stub-session".into());
        if let Some(steps) = &self.episode_steps {
            let changed = state::screen_changed(tab_id, &session_key, screen_hash(screen));
            push_episode_step(
                steps,
                format!(
                    "{} metadata screen_changed changed={changed}",
                    trigger.label()
                ),
                tab_id,
            );
        }
        Ok(TabStateMetadata {
            declared_safe_choice: None,
            tab_snapshot: None,
            process_tree_key: None,
            envelope: None,
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
            session_key,
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
    seen: Arc<Mutex<Option<DecisionQuestionsRequest>>>,
    on_decide: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl StubModel {
    fn answering(answer: Result<DecisionAnswer, DecisionError>) -> Self {
        Self {
            answer,
            calls: Arc::default(),
            seen: Arc::default(),
            on_decide: None,
        }
    }

    fn answering_with_hook(
        answer: Result<DecisionAnswer, DecisionError>,
        on_decide: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            on_decide: Some(Arc::new(on_decide)),
            ..Self::answering(answer)
        }
    }
}

impl DecisionModel for StubModel {
    fn supports_question_batch(&self) -> bool {
        true
    }
    fn id(&self) -> &str {
        "fixture"
    }

    fn token_mode(&self) -> TokenMode {
        TokenMode::Required
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: true,
            score: false,
        }
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        _deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        let _ = request;
        self.answer.clone()
    }

    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        codeterm_decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock() = Some(request.clone());
        if let Some(on_decide) = &self.on_decide {
            on_decide();
        }
        Ok(DecisionResponse {
            answers: request
                .questions
                .iter()
                .map(|(name, spec)| {
                    let answer = match name.as_str() {
                        "label" => self.answer.clone()?,
                        "prompt_visible" => DecisionAnswer::Noul { p: 0.0 },
                        _ => {
                            let codeterm_decision::DecisionQuestion::Choice { options } =
                                &spec.question
                            else {
                                return Err(DecisionError::Unsupported);
                            };
                            choice(
                                if options.contains_key(OTHER) {
                                    OTHER
                                } else {
                                    "__abstain__"
                                },
                                1.0,
                            )
                        }
                    };
                    Ok((name.clone(), answer))
                })
                .collect::<Result<_, DecisionError>>()?,
            usage: None,
        })
    }
}

fn choice(label: &str, confidence: f64) -> DecisionAnswer {
    DecisionAnswer::Choice {
        choice: label.to_owned(),
        probabilities: BTreeMap::from([(label.to_owned(), confidence)]),
        confidence: Some(confidence),
    }
}

fn enabled() -> SupporterConfig {
    SupporterConfig {
        adapter: Some("fixture".into()),
        ..SupporterConfig::default()
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
    )?
    .join()
    .expect("observer thread completes")
}

#[test]
fn supporter_tab_state_not_connected_makes_no_read_or_call() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let (host, calls, _) = StubHost::new("ready", model);
    assert!(spawn_observer(
        &host,
        &SupporterConfig::default(),
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
    assert!(!host._journal_dir.path().join("journal.jsonl").exists());
}

#[test]
fn tab_state_no_call_without_token() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let (mut host, calls, _) = StubHost::new("ready", model);
    host.token = false;
    assert!(spawn_observer(
        &host,
        &enabled(),
        "fixture",
        3000,
        &tab(),
        ObservationTrigger::SpawnReady,
        "fixture-structural-verdict",
    )
    .is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
    assert_eq!(host.runtime.status(0)["calls_last_hour"], 0);
    assert!(!host._journal_dir.path().join("journal.jsonl").exists());
}

#[test]
fn tab_state_context_envelope_contains_all_runtime_fields() {
    let model = StubModel::answering(Ok(choice(WORKING_NORMALLY, 0.91)));
    let (host, calls, seen) = StubHost::new("transcript response", model);
    let tab_id = tab();
    run(&host, &tab_id, ObservationTrigger::WorkerIdle).expect("a valid verdict");

    let request = seen
        .lock()
        .clone()
        .expect("choice adapter received request");
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
    let request = seen
        .lock()
        .clone()
        .expect("choice adapter received request");
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
    let verdict =
        run(&host, &tab(), ObservationTrigger::SpawnReady).expect("healthy spawn was classified");

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
fn tab_state_hook_binding_sequence_keeps_spawn_observation() {
    let tab_id = tab();
    let pty_id = "claude-pty-b57c3888".to_owned();
    let launch_key = state::launch_episode_key(&tab_id, Some(&pty_id), Some(Instant::now()));
    let steps = Arc::new(Mutex::new(Vec::new()));
    push_episode_step(
        &steps,
        "set_tab_agent_type origin=Spawn; agent-launch",
        &tab_id,
    );
    let (mut mux, identity_tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&identity_tab_id, pty_id.clone());
    let identity = {
        let tab = mux
            .get_tab_mut(&identity_tab_id)
            .expect("identity fixture tab");
        tab.agent_type = Some("claude".into());
        tab.agent_set_at = Some(Instant::now());
        Arc::new(Mutex::new(tab.clone()))
    };

    let model_steps = steps.clone();
    let model_tab = tab_id.clone();
    let model_key = launch_key.clone();
    let model_identity = identity.clone();
    let model = StubModel::answering_with_hook(Ok(choice(READY_IDLE, 0.95)), move || {
        push_episode_step(
            &model_steps,
            "launch-readiness observed; SpawnReady claim",
            &model_tab,
        );
        push_episode_step(&model_steps, "hook_session_start received", &model_tab);

        let changed = state::screen_changed(
            &model_tab,
            &model_key,
            screen_hash("Claude session-start screen"),
        );
        push_episode_step(
            &model_steps,
            format!("session-binding event; metadata screen_changed={changed}"),
            &model_tab,
        );

        let hook_key = {
            let mut identity = model_identity.lock();
            let write = crate::agent::provider_identity::apply_provider_identity(
                &mut identity,
                "claude".into(),
                true,
                crate::session::lifecycle::AgentTypeWriteOrigin::Hook,
            );
            assert!(matches!(
                write,
                crate::agent::provider_identity::ProviderIdentityWrite::Applied { .. }
            ));
            state::launch_episode_key(
                &model_tab,
                identity.pty_id.as_deref(),
                identity.agent_set_at,
            )
        };
        assert_eq!(hook_key, model_key);
        push_episode_step(
            &model_steps,
            format!("set_tab_agent_type origin=Hook prev=claude new=claude; key={hook_key}"),
            &model_tab,
        );

        let escalation =
            state::claim_observation(&model_tab, &hook_key, ObservationEvent::StartupEscalation)
                .expect("the existing escalation event can claim this episode");
        push_episode_step(&model_steps, "StartupEscalation trigger claim", &model_tab);
        let action =
            state::apply_observed_verdict(&model_tab, &hook_key, &escalation, READY_IDLE, 0.95)
                .expect("the escalation verdict belongs to its claimed episode");
        assert_eq!(action, SignalAction::WeakHint);
        push_episode_step(
            &model_steps,
            "StartupEscalation verdict applied as one informational hint",
            &model_tab,
        );

        let acknowledgement =
            state::claim_observation(&model_tab, &hook_key, ObservationEvent::SpawnReady);
        assert_eq!(
            acknowledgement,
            Err(state::PermitError::SpawnAlreadyObserved)
        );
        push_episode_step(
            &model_steps,
            "launch-readiness acknowledged on the same PTY",
            &model_tab,
        );
        push_episode_step(
            &model_steps,
            "session-model None -> sonnet; agent ready after 909 ms",
            &model_tab,
        );
        push_episode_step(
            &model_steps,
            "hook_session_start session - -> 943d714a",
            &model_tab,
        );

        state::note_output(&model_tab);
        push_episode_step(
            &model_steps,
            "output_feed::on_output -> note_output",
            &model_tab,
        );
        let changed = state::screen_changed(
            &model_tab,
            &hook_key,
            screen_hash("Claude session-start screen with redraw"),
        );
        push_episode_step(
            &model_steps,
            format!("ServiceContext::metadata -> screen_changed={changed}"),
            &model_tab,
        );
    });
    let (mut host, calls, _) = StubHost::new("Claude initial launch screen", model);
    host.session_key = Some(launch_key);
    host.episode_steps = Some(steps.clone());

    let verdict = observe_guarded(
        &host,
        &enabled(),
        "fixture",
        3000,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "healthy_launch",
    );
    push_episode_step(&steps, "verdict returned; stale check completed", &tab_id);

    let spawn_ok_count = host
        .deliveries
        .lock()
        .iter()
        .filter(|(_, _, action)| *action == SignalAction::SpawnOk)
        .count();
    let stale_discarded = host
        .records
        .lock()
        .iter()
        .any(|line| line.contains("stale observation discarded"));
    assert!(
        verdict.as_ref().is_some_and(|verdict| verdict.label == READY_IDLE)
            && spawn_ok_count == 1,
        "Claude hook-binding launch should yield one spawn-ok; verdict={verdict:?}, spawn_ok_count={spawn_ok_count}, stale_discarded={stale_discarded}\n{}",
        episode_steps_table(&steps),
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn tab_state_opencode_sequence_control() {
    let tab_id = tab();
    let pty_id = "opencode-pty-control";
    let launch_key = state::launch_episode_key(&tab_id, Some(pty_id), Some(Instant::now()));
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.95)));
    let (mut host, calls, _) = StubHost::new("OpenCode ready screen", model);
    host.session_key = Some(launch_key);

    let verdict = observe_guarded(
        &host,
        &enabled(),
        "fixture",
        3000,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "healthy_launch",
    );

    assert_eq!(verdict.expect("OpenCode spawn verdict").label, READY_IDLE);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.deliveries
            .lock()
            .iter()
            .filter(|(_, _, action)| *action == SignalAction::SpawnOk)
            .count(),
        1
    );
    assert!(!host
        .records
        .lock()
        .iter()
        .any(|line| line.contains("stale observation discarded")));
}

#[test]
fn tab_state_codex_sequence_one_spawn_ok() {
    let tab_id = tab();
    let pty_id = "codex-pty-control";
    let launch_key = state::launch_episode_key(&tab_id, Some(pty_id), Some(Instant::now()));
    let output_tab = tab_id.clone();
    let hook_key = launch_key.clone();
    let model = StubModel::answering_with_hook(Ok(choice(READY_IDLE, 0.95)), move || {
        state::screen_changed(
            &output_tab,
            &hook_key,
            screen_hash("Codex session-bound screen"),
        );
        state::note_output(&output_tab);
        state::screen_changed(
            &output_tab,
            &hook_key,
            screen_hash("Codex ready after session binding"),
        );
    });
    let (mut host, calls, _) = StubHost::new("Codex ready screen", model);
    host.session_key = Some(launch_key);

    let verdict = observe_guarded(
        &host,
        &enabled(),
        "fixture",
        3000,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "healthy_launch",
    );

    assert_eq!(verdict.expect("Codex spawn verdict").label, READY_IDLE);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.deliveries
            .lock()
            .iter()
            .filter(|(_, _, action)| *action == SignalAction::SpawnOk)
            .count(),
        1
    );
    assert!(!host
        .records
        .lock()
        .iter()
        .any(|line| line.contains("stale observation discarded")));
}

#[test]
fn supporter_first_opinion_spawn_verdict_discarded_on_nonce_rebinding() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = format!("tab-state-binding-{}", uuid::Uuid::new_v4().simple());
    mux.link_pty(&tab_id, pty_id);
    {
        let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
        tab.agent_type = Some("tab-state-provider".into());
        tab.agent_set_at = Some(Instant::now());
        tab.session_binding_nonce = None;
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("tab-state-provider"),
    ));
    let post_binding_metadata = Arc::new(AtomicUsize::new(0));
    let transition_service = fixture.service.clone();
    let transition_tab = tab_id.clone();
    let observed = post_binding_metadata.clone();
    let model = StubModel::answering_with_hook(Ok(choice(READY_IDLE, 0.95)), move || {
        {
            let mut mux = transition_service.mux.write();
            let tab = mux
                .get_tab_mut(&transition_tab)
                .expect("tab remains live during binding");
            tab.agent_set_at = Some(Instant::now());
            tab.agent_session_id = Some("provider-session".into());
            tab.session_binding_nonce = Some("post-binding-nonce".into());
            tab.reported_session_binding_nonce = Some("post-binding-nonce".into());
        }
        if <ServiceContext as ShadowHost>::metadata(
            &transition_service,
            &transition_tab,
            ObservationTrigger::SpawnReady,
            "ready",
            "ready screen",
        )
        .is_ok()
        {
            observed.fetch_add(1, Ordering::SeqCst);
        }
    });
    let (mut host, calls, _) = StubHost::new("ready screen", model);
    host.metadata_service = Some(fixture.service.clone());

    assert!(run(&host, &tab_id, ObservationTrigger::SpawnReady).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(post_binding_metadata.load(Ordering::SeqCst), 1);
    assert!(host.deliveries.lock().is_empty());
}

#[test]
fn tab_state_spawn_ok_survives_output_during_decide() {
    let tab_id = tab();
    let output_tab = tab_id.clone();
    let model = StubModel::answering_with_hook(Ok(choice(READY_IDLE, 0.95)), move || {
        state::note_output(&output_tab);
    });
    let (host, calls, _) = StubHost::new("ready composer", model);

    let verdict = run(&host, &tab_id, ObservationTrigger::SpawnReady)
        .expect("launch output must not discard the healthy observation");

    assert_eq!(verdict.label, READY_IDLE);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.deliveries.lock().as_slice(),
        &[(
            ObservationTrigger::SpawnReady,
            READY_IDLE.to_owned(),
            SignalAction::SpawnOk,
        )]
    );
    assert!(!host
        .records
        .lock()
        .iter()
        .any(|line| line.contains("stale observation discarded")));
}

#[test]
fn tab_state_verdict_survives_redraw_output() {
    let tab_id = tab();
    let output_tab = tab_id.clone();
    let launch_key = state::launch_episode_key(&tab_id, Some("redraw-pty"), None);
    let redraw_key = launch_key.clone();
    let model = StubModel::answering_with_hook(Ok(choice(ERROR_LOOP, 0.94)), move || {
        state::note_output(&output_tab);
        state::screen_changed(
            &output_tab,
            &redraw_key,
            screen_hash("repeating error spinner 1"),
        );
        state::note_output(&output_tab);
        state::screen_changed(
            &output_tab,
            &redraw_key,
            screen_hash("repeating error retry 2"),
        );
    });
    let (mut host, calls, _) = StubHost::new("repeating error", model);
    host.session_key = Some(launch_key);

    let verdict = observe_guarded(
        &host,
        &enabled(),
        "fixture",
        3000,
        &tab_id,
        ObservationTrigger::WorkerIdle,
        "error_screen",
    )
    .expect("output and screen redraws do not stale a same-episode verdict");

    assert_eq!(verdict.action, SignalAction::WeakHint);
    assert_eq!(
        verdict.classified_frame_hash,
        screen_hash("repeating error")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let frame_hash_record = format!("frame_hash={:016x}", verdict.classified_frame_hash);
    assert!(host
        .records
        .lock()
        .iter()
        .any(|line| line.contains(&frame_hash_record)));
    assert_eq!(
        host.deliveries.lock().as_slice(),
        &[(
            ObservationTrigger::WorkerIdle,
            ERROR_LOOP.to_owned(),
            SignalAction::WeakHint,
        )]
    );
    assert!(!host
        .records
        .lock()
        .iter()
        .any(|line| line.contains("stale observation discarded")));
}

#[test]
fn tab_state_one_spawn_ok_per_launch() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.95)));
    let (host, calls, _) = StubHost::new("ready screen", model);
    let tab_id = tab();

    assert!(run(&host, &tab_id, ObservationTrigger::SpawnReady).is_some());
    assert!(run(&host, &tab_id, ObservationTrigger::SpawnDeadline).is_none());

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.deliveries
            .lock()
            .iter()
            .filter(|(_, _, action)| *action == SignalAction::SpawnOk)
            .count(),
        1
    );
}

#[test]
fn tab_state_two_agreeing_strong_from_distinct_events_acts() {
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
    assert_eq!(
        host.deliveries.lock().as_slice(),
        &[
            (
                ObservationTrigger::WorkerIdle,
                ERROR_LOOP.to_owned(),
                SignalAction::WeakHint,
            ),
            (
                ObservationTrigger::TabExit,
                ERROR_LOOP.to_owned(),
                SignalAction::StrongAction,
            ),
        ]
    );
}

#[test]
fn tab_state_second_agreeing_strong_acts() {
    let tab_id = tab();
    let launch_key = state::launch_episode_key(&tab_id, Some("pty-launch"), None);

    assert_eq!(
        state::apply_verdict(&tab_id, &launch_key, ERROR_LOOP, 0.94),
        SignalAction::WeakHint
    );
    assert_eq!(
        state::apply_verdict(&tab_id, &launch_key, ERROR_LOOP, 0.94),
        SignalAction::StrongAction
    );
}

#[test]
fn tab_state_stuck_prompt_gets_second_observation_from_existing_event() {
    let model = StubModel::answering(Ok(choice(TRUST_OR_PERMISSION_PROMPT, 0.93)));
    let (host, calls, _) = StubHost::new("permission prompt", model);
    let tab_id = tab();

    assert_eq!(
        run(&host, &tab_id, ObservationTrigger::SpawnReady)
            .expect("first prompt observation")
            .action,
        SignalAction::WeakHint
    );
    assert_eq!(
        run(&host, &tab_id, ObservationTrigger::StartupEscalation)
            .expect("existing startup escalation supplies the follow-up")
            .action,
        SignalAction::StrongAction
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn tab_state_proxy_and_bad_model_reach_observation() {
    for (screen, label) in [
        ("connection refused; retrying", ERROR_LOOP),
        ("HTTP 400: unsupported model", MODEL_UNAVAILABLE_OR_QUOTA),
    ] {
        let model = StubModel::answering(Ok(choice(label, 0.94)));
        let (host, calls, _) = StubHost::new(screen, model);
        let verdict = run(&host, &tab(), ObservationTrigger::WorkerIdle)
            .expect("idle-without-report supplies an observation");

        assert_eq!(verdict.label, label);
        assert_eq!(verdict.action, SignalAction::WeakHint);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
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
    assert!(host
        .records
        .lock()
        .iter()
        .any(|line| line.contains("suppressed")));
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

#[path = "connected_tests.rs"]
mod connected_tests;
#[path = "metadata_tests.rs"]
mod metadata_tests;

#[path = "first_opinion_tests.rs"]
mod first_opinion_tests;

#[path = "candidate_tests.rs"]
mod candidate_tests;

#[path = "provenance_tests.rs"]
mod provenance_tests;
