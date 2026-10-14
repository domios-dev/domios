use super::super::{candidates, identity};
use super::*;
use codeterm_types::pty::{PendingPrompt, PromptKind, PromptOption};

fn answer(label: &str, confidence: f64, visible: f64, area: &str) -> DecisionResponse {
    DecisionResponse {
        answers: BTreeMap::from([
            ("label".into(), choice(label, confidence)),
            ("prompt_visible".into(), DecisionAnswer::Noul { p: visible }),
            ("input_area".into(), choice(area, confidence)),
        ]),
        usage: None,
    }
}

#[test]
fn supporter_first_opinion_cold_call_budget_does_not_extend_receipt_authorization() {
    let captured_at = Instant::now();
    let call_started_at = captured_at + std::time::Duration::from_secs(4);
    let receipt_deadline = candidates::receipt_authorization_deadline(captured_at, 3_000);
    let directory = tempfile::tempdir().unwrap();
    let runtime =
        crate::service::supporter::SupporterRuntime::new(directory.path().join("supporter"));
    let call_deadline = runtime.decision_deadline("fixture", 3_000, call_started_at);

    assert_eq!(
        receipt_deadline,
        captured_at + std::time::Duration::from_secs(3)
    );
    assert_eq!(
        call_deadline,
        call_started_at + std::time::Duration::from_secs(5)
    );
    assert!(receipt_deadline < call_started_at);
    assert!(receipt_deadline < call_deadline);
}

#[test]
fn supporter_first_opinion_scrape_model_decides_without_heuristic_veto() {
    for (label, visible, area, live) in [
        (TRUST_OR_PERMISSION_PROMPT, 0.95, "active_modal", true),
        (PROVIDER_UPDATE_PROMPT, 0.95, "active_modal", true),
        (READY_IDLE, 0.05, "composer", false),
        (
            TRUST_OR_PERMISSION_PROMPT,
            0.05,
            "historical_or_draft_text",
            false,
        ),
    ] {
        let (verdict, source) = candidates::select(Ok(answer(label, 0.95, visible, area)));
        assert_eq!(source, identity::DecisionSource::Adapter);
        let verdict = verdict.unwrap();
        assert_eq!(verdict.label, label);
        assert_eq!(verdict.live_prompt(), live);
    }
}

#[test]
fn supporter_first_opinion_scrape_fallback_on_timeout_budget_other_low_confidence() {
    for (result, source) in [
        (
            Err(DecisionError::Deadline),
            identity::DecisionSource::Timeout,
        ),
        (Err(DecisionError::Budget), identity::DecisionSource::Budget),
        (
            Err(DecisionError::Unsupported),
            identity::DecisionSource::error(&DecisionError::Unsupported),
        ),
        (
            Ok(answer(OTHER, 0.99, 0.99, "active_modal")),
            identity::DecisionSource::Other,
        ),
        (
            Ok(answer(
                TRUST_OR_PERMISSION_PROMPT,
                0.59,
                0.99,
                "active_modal",
            )),
            identity::DecisionSource::LowConfidence,
        ),
        (
            Ok(answer(
                TRUST_OR_PERMISSION_PROMPT,
                0.99,
                0.5,
                "active_modal",
            )),
            identity::DecisionSource::LowConfidence,
        ),
        (
            Ok(answer(TRUST_OR_PERMISSION_PROMPT, 0.99, 0.99, OTHER)),
            identity::DecisionSource::Other,
        ),
    ] {
        let (verdict, actual) = candidates::select(result);
        assert!(verdict.is_none());
        assert_eq!(actual, source);
    }
    let mut malformed = answer(TRUST_OR_PERMISSION_PROMPT, 0.99, 0.99, "active_modal");
    malformed.answers.remove("input_area");
    assert_eq!(
        candidates::select(Ok(malformed)).1,
        identity::DecisionSource::error(&DecisionError::Parse("invalid_candidate".into()))
    );
}

#[test]
fn supporter_first_opinion_scrape_no_adapter_identical_zero_work() {
    let (host, calls, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    let callback = Arc::new(AtomicUsize::new(0));
    let called = callback.clone();
    assert!(candidates::spawn(
        &host,
        &SupporterConfig::default(),
        &tab(),
        None,
        move |_, _, _| {
            called.fetch_add(1, Ordering::SeqCst);
        }
    )
    .is_none());
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(callback.load(Ordering::SeqCst), 0);
    assert!(host.records.lock().is_empty());
}

#[test]
fn supporter_first_opinion_scrape_batch_journal_and_disconnect() {
    let (host, calls, seen) = StubHost::new(
        "token=private-value",
        StubModel::answering(Ok(choice(OTHER, 0.99))),
    );
    let callback = Arc::new(AtomicUsize::new(0));
    let called = callback.clone();
    let config = SupporterConfig {
        adapter: Some("fixture".into()),
        ..Default::default()
    };
    let id = tab();
    candidates::spawn(&host, &config, &id, None, move |verdict, source, _| {
        assert!(verdict.is_none());
        assert_eq!(source, identity::DecisionSource::Other);
        called.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap()
    .join()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(callback.load(Ordering::SeqCst), 1);
    let request = seen.lock().clone().unwrap();
    assert_eq!(
        request
            .questions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["input_area", "label", "prompt_visible"]
    );
    assert!(!serde_json::to_string(&request)
        .unwrap()
        .contains("private-value"));
    let journal = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    let record = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["action"] == "classification")
        .unwrap();
    assert_eq!(record["decided_by"], "fallback(other)");
    assert!(record["observation_id"].as_u64().unwrap() > 0);
    assert!(host.deliveries.lock().is_empty());
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);

    let registry = Arc::new(SupporterRegistry::new());
    let remove = registry.clone();
    let (mut host, _, _) = StubHost::new(
        "opaque",
        StubModel::answering_with_hook(Ok(choice(READY_IDLE, 0.99)), move || {
            remove.unregister("fixture")
        }),
    );
    registry.register("fixture", host.models.get("fixture").unwrap());
    host.models = registry;
    candidates::spawn(&host, &config, &tab(), None, |_, _, _| {
        panic!("disconnected verdict must be discarded")
    })
    .unwrap()
    .join()
    .unwrap();
}

fn prompt() -> PendingPrompt {
    PendingPrompt {
        request_id: None,
        question: "Fixture selection".into(),
        options: vec![
            PromptOption {
                label: "Opaque alpha".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "Opaque beta".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: None,
    }
}

fn update_prompt() -> PendingPrompt {
    let mut update = prompt();
    update.options[0].label = "Update".into();
    update.options[1].label = "Skip".into();
    update
}

#[test]
fn supporter_first_opinion_scrape_a004b77d4_transcript_and_composer_copy_negatives() {
    let live = "Fixture selection\n❯ 1. Opaque alpha\n2. Opaque beta\nEnter to confirm\n";
    let transcript = format!("{live}Selected alpha\nCompleted response\n❯\n");
    let draft = "❯ Fixture selection\n1. Opaque alpha\n2. Opaque beta\nEnter to confirm\n";
    let captured =
        include_str!("../../agent/state_reactor/fixtures/codex_transcript_with_composer.txt");
    let (verdict, _) = candidates::select(Ok(answer(
        TRUST_OR_PERMISSION_PROMPT,
        0.99,
        0.99,
        "active_modal",
    )));
    assert!(candidates::allows_prompt(
        verdict.as_ref().unwrap(),
        live,
        &prompt()
    ));
    assert!(!candidates::allows_prompt(
        verdict.as_ref().unwrap(),
        &transcript,
        &prompt()
    ));
    let mut composer = prompt();
    composer.question = "❯ Fixture selection".into();
    assert!(!candidates::allows_prompt(
        verdict.as_ref().unwrap(),
        draft,
        &composer
    ));
    assert!(!candidates::allows_prompt(
        verdict.as_ref().unwrap(),
        "Fixture selection",
        &prompt()
    ));
    let mut captured_prompt = prompt();
    captured_prompt.options = vec![
        PromptOption {
            label: "opaque-command-alpha".into(),
            value: "1".into(),
        },
        PromptOption {
            label: "opaque-command-beta".into(),
            value: "2".into(),
        },
    ];
    captured_prompt.selected_values = Some(vec!["1".into()]);
    assert!(!candidates::allows_prompt(
        verdict.as_ref().unwrap(),
        captured,
        &captured_prompt
    ));
}

#[test]
fn supporter_first_opinion_scrape_service_persists_only_evidenced_prompt() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "fixture-pty".into());
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider")
            .with_parsed_prompt(prompt()),
    ));
    let mut snapshot = codeterm_plugin::tab_state::TabStateSnapshot { tab_id: id.clone(), agent_type: "fixture-provider".into(), session_id: None, cwd: None, screen_text: "Fixture selection\n❯ 1. Opaque alpha\n2. Opaque beta\nEnter to confirm\nSelected alpha\n❯\n".into() };
    let (verdict, _) = candidates::select(Ok(answer(
        TRUST_OR_PERMISSION_PROMPT,
        0.99,
        0.99,
        "active_modal",
    )));
    let verdict = verdict.unwrap();
    fixture
        .service
        .publish_model_scrape_candidate(&snapshot, &verdict, &|| true);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pending_prompt
        .is_none());
    snapshot.screen_text =
        "Fixture selection\n❯ 1. Opaque alpha\n2. Opaque beta\nEnter to confirm\n".into();
    fixture
        .service
        .publish_model_scrape_candidate(&snapshot, &verdict, &|| true);
    let tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    assert_eq!(tab.pending_prompt, Some(prompt()));
    assert_eq!(tab.pending_prompt_kind, Some(PromptKind::Provider));
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test("fixture-pty"),
        0
    );
}

#[test]
fn supporter_first_opinion_safe_response_provider_choice_in_same_redacted_batch() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "safe-choice-batch-pty".into());
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.role = Some(codeterm_types::agent::AgentRole::Worker);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut update = update_prompt();
    update.options[1].value = "opaque-skip-value".into();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider")
            .with_parsed_prompt(update)
            .with_classified_prompt_auto_response(
                codeterm_plugin::provider::ClassifiedPromptOptionRole::Skip,
            )
            .with_live_prompt_response(1, vec![b"opaque-step".to_vec()]),
    ));
    let (mut host, calls, seen) = StubHost::new(
        "Fixture selection\n❯ 1. Update\n2. Skip\nEnter to confirm\n",
        StubModel::answering(Ok(choice(OTHER, 0.99))),
    );
    host.metadata_service = Some(fixture.service.clone());
    let config = SupporterConfig {
        adapter: Some("fixture".into()),
        ..Default::default()
    };
    candidates::spawn(&host, &config, &id, None, |_, _, _| {})
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let request = seen.lock().clone().unwrap();
    assert_eq!(
        request
            .questions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["input_area", "label", "prompt_visible", "safe_choice"]
    );
    let codeterm_decision::DecisionQuestion::Choice { options } =
        &request.questions["safe_choice"].question
    else {
        panic!("choice");
    };
    assert!(options.contains_key("opaque-skip-value"));
    assert!(options.contains_key(OTHER));
    assert!(!serde_json::to_string(&request)
        .unwrap()
        .contains("private-value"));
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pending_prompt
        .is_none());
    let redacted_id = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&id).unwrap();
        let tab_id = mux.create_tab(&workspace).unwrap();
        mux.link_pty(&tab_id, "redacted-safe-choice-pty".into());
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.role = Some(codeterm_types::agent::AgentRole::Worker);
        tab_id
    };
    let (mut redacted, redacted_calls, seen) = StubHost::new(
        "Fixture selection\n❯ 1. Update\n2. Skip\nEnter to confirm\ntoken=private-value",
        StubModel::answering(Ok(choice(OTHER, 0.99))),
    );
    redacted.metadata_service = Some(fixture.service.clone());
    candidates::spawn(&redacted, &config, &redacted_id, None, |_, _, _| {})
        .unwrap()
        .join()
        .unwrap();
    let request = seen.lock().clone().unwrap();
    assert_eq!(redacted_calls.load(Ordering::SeqCst), 1);
    assert!(!request.questions.contains_key("safe_choice"));
    assert!(!serde_json::to_string(&request)
        .unwrap()
        .contains("private-value"));
    assert!(candidates::spawn(&redacted, &config, &redacted_id, None, |_, _, _| {}).is_none());
    assert_eq!(redacted_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn supporter_first_opinion_is_admitted_once_per_launch_after_periodic_observation() {
    let (mut host, calls, _) = StubHost::new(
        "opaque screen",
        StubModel::answering(Ok(choice(OTHER, 0.99))),
    );
    let id = tab();
    let config = enabled();
    run(&host, &id, ObservationTrigger::WorkerIdle).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(candidates::spawn(&host, &config, &id, None, |_, _, _| {}).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(candidates::spawn(&host, &config, &id, None, |_, _, _| {}).is_none());
    assert!(candidates::spawn_with_origin(
        &host,
        &config,
        &id,
        None,
        ObservationTrigger::ParkedEdge,
        None,
        None,
        |_, _, _| {},
    )
    .is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    host.session_key = Some("next-launch".into());
    candidates::spawn(&host, &config, &id, None, |_, _, _| {})
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

struct CandidateModel {
    response: Result<DecisionResponse, DecisionError>,
    hook: Option<Box<dyn Fn() + Send + Sync>>,
}

#[test]
fn supporter_first_opinion_safe_response_bound_receipt_cancels_user_nonce_and_disconnect() {
    for arm in 0..3 {
        let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
        let pty = format!("safe-response-{}", uuid::Uuid::new_v4());
        mux.link_pty(&id, pty.clone());
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.pending_prompt = Some(update_prompt());
        tab.pending_prompt_kind = Some(PromptKind::ManualUpdate);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture-provider")
                .with_parsed_prompt(update_prompt())
                .with_classified_prompt_auto_response(
                    codeterm_plugin::provider::ClassifiedPromptOptionRole::Skip,
                )
                .with_live_prompt_response(1, vec![b"opaque-step".to_vec()]),
        ));
        fixture
            .service
            .prompt_detector
            .lock()
            .hook_detected_scraped(&id, update_prompt());
        let (mut host, _, _) = StubHost::new(
            "Fixture selection\n❯ 1. Update\n2. Skip\nEnter to confirm\n",
            StubModel::answering(Ok(choice(OTHER, 0.99))),
        );
        host.metadata_service = Some(fixture.service.clone());
        let mut response = answer(PROVIDER_UPDATE_PROMPT, 0.95, 0.95, "active_modal");
        response
            .answers
            .insert("safe_choice".into(), choice("2", 0.96));
        host.models.register(
            "fixture",
            Arc::new(CandidateModel {
                response: Ok(response),
                hook: None,
            }),
        );
        let selected = Arc::new(Mutex::new(None));
        let save = selected.clone();
        let config = SupporterConfig {
            adapter: Some("fixture".into()),
            ..Default::default()
        };
        candidates::spawn(&host, &config, &id, None, move |verdict, source, valid| {
            assert!(valid());
            assert_eq!(source, identity::DecisionSource::Adapter);
            *save.lock() = verdict;
        })
        .unwrap()
        .join()
        .unwrap();
        let verdict = selected.lock().clone().unwrap();
        let receipt = verdict.receipt.unwrap();
        let envelope = receipt.metadata.envelope.as_ref().unwrap();
        let ledgers = envelope.ledgers.clone();
        assert_eq!(
            receipt.deadline,
            candidates::receipt_authorization_deadline(envelope.captured_at, config.deadline_ms)
        );
        assert!(receipt.current(&host, &id, &ledgers));
        let mut expired = receipt.clone();
        expired.deadline = Instant::now();
        assert!(!expired.current(&host, &id, &ledgers));
        match arm {
            0 => fixture.service.prompt_detector.lock().mark_responded(&id),
            1 => {
                fixture
                    .service
                    .mux
                    .write()
                    .get_tab_mut(&id)
                    .unwrap()
                    .session_binding_nonce = Some("nonce-b".into())
            }
            _ => host.models.unregister("fixture"),
        }
        assert!(!receipt.current(&host, &id, &ledgers));
        assert_eq!(
            crate::service::output::prompt_step_dispatch_count_for_test(&pty),
            0
        );
        assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    }
}

impl DecisionModel for CandidateModel {
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
    fn supports_question_batch(&self) -> bool {
        true
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        Err(DecisionError::Unsupported)
    }
    fn decide_questions(
        &self,
        _: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        codeterm_decision::physical::before_request()?;
        if let Some(hook) = &self.hook {
            hook();
        }
        self.response.clone()
    }
}

#[test]
fn supporter_first_opinion_scrape_journal_every_model_and_fallback_arm() {
    for (response, expected) in [
        (
            Ok(answer(
                TRUST_OR_PERMISSION_PROMPT,
                0.95,
                0.95,
                "active_modal",
            )),
            "adapter",
        ),
        (Ok(answer(READY_IDLE, 0.95, 0.05, "composer")), "adapter"),
        (
            Ok(answer(OTHER, 0.95, 0.95, "active_modal")),
            "fallback(other)",
        ),
        (
            Ok(answer(
                TRUST_OR_PERMISSION_PROMPT,
                0.59,
                0.95,
                "active_modal",
            )),
            "fallback(low_confidence)",
        ),
        (Err(DecisionError::Deadline), "fallback(timeout)"),
        (Err(DecisionError::Budget), "fallback(budget)"),
        (
            Err(DecisionError::Transport("private-value".into())),
            "fallback(error)",
        ),
    ] {
        let (host, _, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
        host.models.register(
            "fixture",
            Arc::new(CandidateModel {
                response,
                hook: None,
            }),
        );
        let config = SupporterConfig {
            adapter: Some("fixture".into()),
            ..Default::default()
        };
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let count = fallback_calls.clone();
        candidates::spawn(
            &host,
            &config,
            &tab(),
            None,
            move |verdict, source, valid| {
                assert!(valid());
                assert_eq!(source.as_str(), expected);
                if source == identity::DecisionSource::Adapter {
                    assert!(verdict.is_some());
                } else {
                    count.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .unwrap()
        .join()
        .unwrap();
        assert_eq!(
            fallback_calls.load(Ordering::SeqCst),
            usize::from(expected != "adapter")
        );
        let journal =
            std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
        let row = journal
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|row| row["action"] == "classification")
            .unwrap();
        assert_eq!(row["decided_by"], expected);
        assert!(row["observation_id"].as_u64().unwrap() > 0);
        assert!(!journal.contains("private-value"));
        assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn supporter_first_opinion_scrape_stale_capture_and_generation_never_runs_consumer() {
    let (host, calls, _) = StubHost::new("current", StubModel::answering(Ok(choice(OTHER, 0.99))));
    let config = SupporterConfig {
        adapter: Some("fixture".into()),
        ..Default::default()
    };
    assert!(
        candidates::spawn(&host, &config, &tab(), Some("previous"), |_, _, _| panic!(
            "stale capture"
        ))
        .is_none()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let id = tab();
    let replaced = id.clone();
    host.models.register(
        "fixture",
        Arc::new(CandidateModel {
            response: Ok(answer(
                TRUST_OR_PERMISSION_PROMPT,
                0.95,
                0.95,
                "active_modal",
            )),
            hook: Some(Box::new(move || {
                super::super::coordinator::global().observe(
                    &replaced,
                    "new-launch-nonce",
                    None,
                    99,
                );
            })),
        }),
    );
    candidates::spawn(&host, &config, &id, None, |_, _, _| {
        panic!("stale generation")
    })
    .unwrap()
    .join()
    .unwrap();
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn supporter_first_opinion_unknown_launch_block_uses_input_area_evidence() {
    let (host, _, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    let (request, _) = prepare_observation(
        &host,
        &tab(),
        ObservationTrigger::StartupEscalation,
        "unknown_launch_block",
    )
    .unwrap();
    for key in ["label", "prompt_visible", "input_area"] {
        assert!(request.questions.contains_key(key));
    }
    for area in ["composer", "historical_or_draft_text", OTHER] {
        let verdict =
            observation_answers(answer(TRUST_OR_PERMISSION_PROMPT, 0.99, 0.99, area)).unwrap();
        assert_eq!(verdict.prompt_visible, 0.0);
    }
}

#[test]
fn supporter_first_opinion_scrape_explicit_handle_wins_and_failed_revalidation_abstains() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "fixture-explicit-pty".into());
    mux.get_tab_mut(&id).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider")
            .with_parsed_prompt(prompt()),
    ));
    let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
        tab_id: id.clone(),
        agent_type: "fixture-provider".into(),
        session_id: None,
        cwd: None,
        screen_text: "Fixture selection\n❯ 1. Opaque alpha\n2. Opaque beta\nEnter to confirm\n"
            .into(),
    };
    let (verdict, _) = candidates::select(Ok(answer(
        TRUST_OR_PERMISSION_PROMPT,
        0.99,
        0.99,
        "active_modal",
    )));
    fixture
        .service
        .publish_model_scrape_candidate(&snapshot, &verdict.unwrap(), &|| false);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pending_prompt
        .is_none());
    let explicit = prompt();
    let decision = fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&id, explicit.clone());
    fixture
        .service
        .apply_prompt_decision_with_kind(&decision, None, PromptKind::Structured, true);
    let (negative, _) = candidates::select(Ok(answer(READY_IDLE, 0.99, 0.01, "composer")));
    fixture
        .service
        .publish_model_scrape_candidate(&snapshot, &negative.unwrap(), &|| true);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .pending_prompt,
        Some(explicit)
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .pending_prompt_kind,
        Some(PromptKind::Structured)
    );
}

#[test]
fn supporter_first_opinion_scrape_service_user_answer_and_reused_pty_new_nonce_cancel() {
    for user_response in [true, false] {
        let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
        mux.link_pty(&id, "reused-pty".into());
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture-provider"),
        ));
        fixture
            .service
            .prompt_detector
            .lock()
            .hook_detected_scraped(&id, prompt());
        let service = fixture.service.clone();
        let changed_tab = id.clone();
        let (mut host, _, _) = StubHost::new(
            "unchanged frame",
            StubModel::answering(Ok(choice(OTHER, 0.99))),
        );
        host.metadata_service = Some(fixture.service.clone());
        host.models.register(
            "fixture",
            Arc::new(CandidateModel {
                response: Ok(answer(
                    TRUST_OR_PERMISSION_PROMPT,
                    0.95,
                    0.95,
                    "active_modal",
                )),
                hook: Some(Box::new(move || {
                    if user_response {
                        service.prompt_detector.lock().mark_responded(&changed_tab);
                    } else {
                        service
                            .mux
                            .write()
                            .get_tab_mut(&changed_tab)
                            .unwrap()
                            .session_binding_nonce = Some("nonce-2".into());
                    }
                })),
            }),
        );
        let config = SupporterConfig {
            adapter: Some("fixture".into()),
            ..Default::default()
        };
        candidates::spawn(&host, &config, &id, None, |_, _, _| {
            panic!("stale consumer")
        })
        .unwrap()
        .join()
        .unwrap();
        assert_eq!(host.writes.load(Ordering::SeqCst), 0);
        assert!(host.deliveries.lock().is_empty());
    }
}

#[test]
fn supporter_first_opinion_scrape_saved_connections_never_choose_a_fallback_adapter() {
    for active in [None, Some(String::new()), Some("unregistered".into())] {
        let (host, calls, _) =
            StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
        let config = SupporterConfig {
            adapter: active,
            connections: BTreeMap::from([(
                "fixture".into(),
                codeterm_types::config::SupporterConnection {
                    model: None,
                    endpoint: Some("http://localhost:1234".into()),
                },
            )]),
            ..Default::default()
        };
        assert!(!super::super::observer::connected(
            &host, &config, "fixture"
        ));
        assert!(
            candidates::spawn(&host, &config, &tab(), None, |_, _, _| panic!(
                "inactive connection consumer"
            ))
            .is_none()
        );
        assert_eq!(host.reads.load(Ordering::SeqCst), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(host.writes.load(Ordering::SeqCst), 0);
        assert!(host.records.lock().is_empty());
    }
}

#[test]
fn supporter_first_opinion_scrape_unapplied_active_model_is_not_connected() {
    let (host, calls, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    let config = SupporterConfig {
        adapter: Some("fixture".into()),
        connections: BTreeMap::from([(
            "fixture".into(),
            codeterm_types::config::SupporterConnection {
                model: Some("unapplied-model".into()),
                endpoint: None,
            },
        )]),
        ..Default::default()
    };
    assert!(
        candidates::spawn(&host, &config, &tab(), None, |_, _, _| panic!(
            "unapplied model consumer"
        ))
        .is_none()
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(host.records.lock().is_empty());
}

#[test]
fn captured_picker_supporter_admission_uses_modal_shape_without_permission_category() {
    use crate::agent::provider::AgentProvider;
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../../plugins/provider-codex/plugin.json"
    ))
    .unwrap();
    let vm = PluginVm::from_source(
        "capture",
        include_str!("../../../../plugins/provider-codex/plugin.js").into(),
    )
    .unwrap();
    let provider = JsProvider::new(manifest, Arc::new(vm));
    let screen = include_str!("../../../../plugins/provider-codex/tests/fixtures/tab-state/live-model-picker-msg07-result.txt");
    let prompt = provider
        .parse_prompt(screen)
        .expect("captured seven-choice picker");
    for label in [READY_IDLE, WORKING_NORMALLY, TRUST_OR_PERMISSION_PROMPT] {
        let (verdict, source) = candidates::select(Ok(answer(label, 0.99, 0.99, "active_modal")));
        assert_eq!(source, identity::DecisionSource::Adapter);
        assert!(candidates::allows_prompt(
            verdict.as_ref().unwrap(),
            screen,
            &prompt
        ));
        assert!(!candidates::allows_prompt(
            verdict.as_ref().unwrap(),
            &format!("{screen}\n› composer"),
            &prompt
        ));
        let (verdict, _) = candidates::select(Ok(answer(label, 0.99, 0.99, "composer")));
        assert!(!candidates::allows_prompt(
            verdict.as_ref().unwrap(),
            screen,
            &prompt
        ));
    }
}
