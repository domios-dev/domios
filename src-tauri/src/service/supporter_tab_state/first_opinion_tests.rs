use super::super::{coordinator, identity, privacy, scheduler};
use super::*;
use codeterm_types::decision::DecisionError;
use std::sync::mpsc;

#[test]
fn supporter_first_opinion_secrets_redacted_before_model_state() {
    let secret = "opaque-secret";
    let code = "918273";
    let screen = format!("token={secret}\nAuthorization: Bearer {secret}\nhttps://example.test/login?code={code}&token={secret}\nverification code: {code}");
    let (host, _, seen) = StubHost::new(&screen, StubModel::answering(Ok(choice(OTHER, 0.95))));
    run(&host, &tab(), ObservationTrigger::StartupEscalation).unwrap();
    let serialized = serde_json::to_string(&seen.lock().clone().unwrap()).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains(code));
    let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    assert!(!text.contains(secret));
    assert!(!text.contains(code));
    let bounded = privacy::screen(&"🙂".repeat(5000));
    assert!(bounded.len() <= 8192);
    assert!(privacy::complete_screen("opaque-a\r\nopaque-b\r\n"));
    assert!(!privacy::complete_screen(&screen));
    assert!(!privacy::complete_screen(&"opaque\n".repeat(41)));
}

#[test]
fn supporter_first_opinion_journal_decided_by_per_arm() {
    for (answer, expected) in [
        (Ok(choice(ERROR_LOOP, 0.60)), "adapter"),
        (Ok(choice(ERROR_LOOP, 0.59)), "fallback(low_confidence)"),
        (Ok(choice(OTHER, 0.99)), "fallback(other)"),
        (Err(DecisionError::Deadline), "fallback(timeout)"),
        (Err(DecisionError::Budget), "fallback(budget)"),
        (
            Err(DecisionError::Transport("private detail".into())),
            "fallback(error)",
        ),
    ] {
        let (host, _, _) = StubHost::new("opaque", StubModel::answering(answer));
        run(&host, &tab(), ObservationTrigger::WorkerIdle);
        let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
        let decision: Value = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|line| line["action"] == "classification")
            .unwrap();
        assert_eq!(decision["decided_by"], expected);
        assert!(decision["observation_id"].as_u64().unwrap() > 0);
        assert_eq!(decision["asked"].as_array().unwrap().len(), 1);
        assert!(decision["asked"][0]["question"]
            .as_str()
            .unwrap()
            .contains("Fixture worker"));
        assert!(!text.contains("private detail"));
    }
    assert_eq!(
        identity::DecisionSource::NoAdapter.as_str(),
        "fallback(no_adapter)"
    );
}

#[test]
fn supporter_first_opinion_adapter_disconnect_cancels_result() {
    let registry = Arc::new(SupporterRegistry::new());
    let remove = registry.clone();
    let model = StubModel::answering_with_hook(Ok(choice(ERROR_LOOP, 0.99)), move || {
        remove.unregister("fixture")
    });
    let (mut host, _, _) = StubHost::new("opaque", model);
    registry.register("fixture", host.models.get("fixture").unwrap());
    host.models = registry;
    assert!(run(&host, &tab(), ObservationTrigger::WorkerIdle).is_none());
    assert!(host.deliveries.lock().is_empty());
}

#[test]
fn supporter_first_opinion_observation_ids_unique() {
    let (host, _, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    run(&host, &tab(), ObservationTrigger::WorkerIdle);
    run(&host, &tab(), ObservationTrigger::WorkerIdle);
    let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    let ids: Vec<_> = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["action"] == "classification")
        .map(|line| line["observation_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn supporter_first_opinion_scheduler_bounds_coalesces_and_never_waits_for_model() {
    let queue = scheduler::Scheduler::new();
    let (started_tx, started_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut handles = Vec::new();
    for index in 0..4 {
        let (tx, rx) = mpsc::channel();
        releases.push(tx);
        let started = started_tx.clone();
        handles.push(
            queue
                .submit(format!("active-{index}"), false, 2, move || {
                    started.send(()).unwrap();
                    rx.recv().unwrap();
                    None
                })
                .unwrap(),
        );
    }
    for _ in 0..4 {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    let extra = queue.submit("active-0".into(), false, 2, || None).unwrap();
    assert_eq!(queue.status().active, 4);
    assert_eq!(queue.status().queued, 1);
    let replacement = queue.submit("active-0".into(), false, 2, || None).unwrap();
    assert_eq!(queue.status().queued, 1);
    assert!(extra.join().unwrap().is_none());
    for index in 0..31 {
        queue
            .submit(format!("queued-{index}"), false, 2, || None)
            .unwrap();
    }
    assert_eq!(queue.status().queued, 32);
    assert!(queue.submit("critical".into(), true, 2, || None).is_some());
    assert_eq!(queue.status().queued, 32);
    assert!(queue
        .submit("oversized".into(), false, 7, || None)
        .is_none());
    for release in releases {
        release.send(()).unwrap();
    }
    for handle in handles {
        handle.join().unwrap();
    }
    replacement.join().unwrap();
    queue.close();
}

#[test]
fn supporter_first_opinion_action_ticket_single_owner_across_ledgers() {
    let owner = coordinator::ActionOwner::default();
    let ledgers = coordinator::LedgerSnapshot::default();
    let first = owner.observe("tab", "nonce-1", Some(11), 22);
    let same = owner.observe("tab", "nonce-1", Some(11), 23);
    assert_eq!(first.occurrence, same.occurrence);
    assert!(!owner.is_current(&first));
    let ticket = owner.claim(&same, ledgers.clone()).unwrap();
    for _ in 0..4 {
        assert!(owner.claim(&same, ledgers.clone()).is_none());
    }
    assert!(owner.ticket_is_current(&ticket, &ledgers));
    for ledger in [0, 1, 2, 3] {
        let mut responded = ledgers.clone();
        match ledger {
            0 => responded.trust = 1,
            1 => responded.update = 1,
            2 => responded.permission = 1,
            _ => responded.supporter = 1,
        }
        assert!(!owner.ticket_is_current(&ticket, &responded));
    }
    owner.observe("tab", "nonce-1", None, 24);
    assert!(!owner.ticket_is_current(&ticket, &ledgers));
    let next = owner.observe("tab", "nonce-1", Some(11), 22);
    assert_ne!(next.occurrence, first.occurrence);
    assert!(owner.claim(&next, ledgers).is_some());
    owner.cancel("tab");
    assert!(!owner.is_current(&next));
}

#[test]
fn supporter_first_opinion_safe_response_fresh_observation_in_same_occurrence_has_one_ticket() {
    let owner = coordinator::ActionOwner::default();
    let ledgers = coordinator::LedgerSnapshot::default();
    let first = owner.observe("tab", "launch", Some(10), 20);
    let ticket = owner.claim(&first, ledgers.clone()).unwrap();
    assert!(owner.claim(&first, ledgers.clone()).is_none());
    let second = owner.observe("tab", "launch", Some(10), 21);
    assert_eq!(first.occurrence, second.occurrence);
    assert!(!owner.ticket_is_current(&ticket, &ledgers));
    assert!(owner.claim(&second, ledgers.clone()).is_some());
    assert!(owner.claim(&second, ledgers).is_none());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_reused_pty_new_nonce_and_user_answer_rejected() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab_id, "reused-pty".into());
    mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some("nonce-1".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let before =
        identity::LocalIdentity::of_tab(fixture.service.mux.read().get_tab(&tab_id).unwrap());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some("nonce-2".into());
    assert!(!fixture
        .service
        .observation_identity_is_current(&tab_id, &before));
    let current =
        identity::LocalIdentity::of_tab(fixture.service.mux.read().get_tab(&tab_id).unwrap());
    assert_ne!(before.launch_key, current.launch_key);
    assert!(fixture
        .service
        .observation_identity_is_current(&tab_id, &current));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = Some("new-session".into());
    assert!(!fixture
        .service
        .observation_identity_is_current(&tab_id, &current));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_service_ledgers_are_part_of_action_ticket() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let before = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    let owner = coordinator::ActionOwner::default();
    let observation = owner.observe(&tab_id, "nonce", Some(11), 22);
    let ticket = owner.claim(&observation, before.clone()).unwrap();
    assert!(crate::service::output::claim_manual_update_occurrence(
        &tab_id,
        "occurrence"
    ));
    let after = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    assert_ne!(before.update, after.update);
    assert!(!owner.ticket_is_current(&ticket, &after));
    crate::service::output::clear_manual_update_occurrence(&tab_id);
}

#[test]
fn supporter_first_opinion_model_first_selection_defers_the_heuristic_per_arm() {
    let calls = AtomicUsize::new(0);
    let (value, source) = identity::select(Some(Ok(("adapter", 0.60, false))), || {
        calls.fetch_add(1, Ordering::SeqCst);
        "heuristic"
    });
    assert_eq!(
        (value, source),
        ("adapter", identity::DecisionSource::Adapter)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for (answer, expected) in [
        (None, identity::DecisionSource::NoAdapter),
        (
            Some(Ok(("adapter", 0.59, false))),
            identity::DecisionSource::LowConfidence,
        ),
        (
            Some(Ok(("adapter", 0.99, true))),
            identity::DecisionSource::Other,
        ),
        (
            Some(Err(DecisionError::Deadline)),
            identity::DecisionSource::Timeout,
        ),
        (
            Some(Err(DecisionError::Budget)),
            identity::DecisionSource::Budget,
        ),
        (
            Some(Err(DecisionError::Unsupported)),
            identity::DecisionSource::error(&DecisionError::Unsupported),
        ),
    ] {
        let before = calls.load(Ordering::SeqCst);
        let (value, source) = identity::select(answer, || {
            calls.fetch_add(1, Ordering::SeqCst);
            "heuristic"
        });
        assert_eq!((value, source), ("heuristic", expected));
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        let id = state::next_observation_id();
        let journal = identity::journal("tab", id, source, OTHER, None, "supplied-title");
        assert_eq!(journal["decided_by"], expected.as_str());
        assert_eq!(journal["observation_id"], id);
    }
}

#[test]
fn supporter_first_opinion_stale_cancellation_preserves_newer_observation() {
    let owner = coordinator::ActionOwner::default();
    let stale = owner.observe("tab", "nonce", Some(1), 1);
    let current = owner.observe("tab", "nonce", Some(1), 2);
    owner.cancel_observation(&stale);
    assert!(owner.is_current(&current));
    owner.cancel_observation(&current);
    assert!(!owner.is_current(&current));
    let fresh = owner.observe("tab", "nonce", Some(1), 2);
    assert!(owner
        .claim(&fresh, coordinator::LedgerSnapshot::default())
        .is_some());
}

#[test]
fn supporter_first_opinion_all_named_questions_fit_one_bounded_envelope() {
    let (host, _, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    let (_, mut metadata) =
        prepare_observation(&host, &tab(), ObservationTrigger::WorkerIdle, "opaque").unwrap();
    metadata.context["pending_prompt_kind"] = serde_json::json!("manual_update");
    metadata.declared_safe_choice = Some("b".into());
    metadata.prompt_options = vec![
        NumberedPromptOption {
            number: 1,
            label: "opaque-a".into(),
            value: "a".into(),
        },
        NumberedPromptOption {
            number: 2,
            label: "opaque-b".into(),
            value: "b".into(),
        },
    ];
    metadata.context["observation_event"] = serde_json::json!("anomaly");
    let questions =
        questions::observation_questions(&metadata, safe_choice::safe_choice_question(&metadata));
    assert_eq!(questions.len(), scheduler::MAX_QUESTIONS);
    assert_eq!(
        questions.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "input_area",
            "label",
            "progress",
            "prompt_visible",
            "provider_failure",
            "safe_choice"
        ]
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_user_response_changes_prompt_receipt_without_a_redraw() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let prompt = codeterm_types::pty::PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: vec![],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&tab_id, prompt.clone());
    let before = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    fixture
        .service
        .prompt_detector
        .lock()
        .mark_responded(&tab_id);
    let after = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    assert_ne!(before.prompt, after.prompt);
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&tab_id, prompt);
    let next = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    assert_ne!(before.prompt, next.prompt);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_fixture_cancels_after_user_response_without_a_redraw() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.session_binding_nonce = Some("nonce".into());
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider"),
    ));
    let prompt = codeterm_types::pty::PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: vec![],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&tab_id, prompt);
    let service = fixture.service.clone();
    let response_tab = tab_id.clone();
    let model = StubModel::answering_with_hook(Ok(choice(ERROR_LOOP, 0.99)), move || {
        service.prompt_detector.lock().mark_responded(&response_tab);
    });
    let (mut host, calls, _) = StubHost::new("unchanged opaque screen", model);
    host.metadata_service = Some(fixture.service.clone());
    assert!(run(&host, &tab_id, ObservationTrigger::WorkerIdle).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(host.deliveries.lock().is_empty());
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn supporter_first_opinion_adapter_policy_changes_invalidate_the_receipt() {
    let config = enabled();
    let before = identity::supporter_policy(&config);
    for index in 0..4 {
        let mut changed = config.clone();
        match index {
            0 => {
                changed
                    .connections
                    .entry("fixture".into())
                    .or_default()
                    .model = Some("opaque-model".into());
            }
            1 => {
                changed
                    .connections
                    .entry("fixture".into())
                    .or_default()
                    .endpoint = Some("https://example.test/opaque".into());
            }
            2 => changed.calls_per_hour = Some(1),
            _ => changed.deadline_ms += 1,
        }
        assert_ne!(before, identity::supporter_policy(&changed));
    }
}

#[test]
fn supporter_first_opinion_narrative_and_observation_survive_journal_and_feed() {
    let private_screen = "supplied-screen-fixture";
    let (host, _, _) = StubHost::new(
        private_screen,
        StubModel::answering(Ok(choice(ERROR_LOOP, 0.95))),
    );
    let tab_id = tab();
    run(&host, &tab_id, ObservationTrigger::WorkerIdle).unwrap();
    run(&host, &tab_id, ObservationTrigger::PromptConfirmation).unwrap();
    let feed = host.runtime.feed(100, None).unwrap();
    let observations: Vec<_> = feed
        .iter()
        .filter(|row| row["observation_id"].is_u64())
        .collect();
    assert_eq!(observations.len(), 2);
    assert_ne!(
        observations[0]["observation_id"],
        observations[1]["observation_id"]
    );
    for row in &observations {
        assert_eq!(row["decided_by"], "adapter");
        assert_eq!(row["asked"].as_array().unwrap().len(), 1);
        assert!(row["asked"][0]["question"]
            .as_str()
            .unwrap()
            .contains("Fixture worker"));
    }
    for row in feed.iter().filter(|row| row["action"] == "strong_action") {
        assert_eq!(row["asked"].as_array().unwrap().len(), 1);
        assert!(row["asked"][0]["question"]
            .as_str()
            .unwrap()
            .contains("Fixture worker"));
    }
    assert!(feed.iter().any(|row| row["action"] == "strong_action"));
    let verdict = feed
        .iter()
        .find(|row| {
            row["observation_id"].is_null()
                && row["reason"] == "model_answer"
                && row["asked"]
                    .as_array()
                    .is_some_and(|asked| asked.len() == 2)
        })
        .unwrap();
    let pairs = verdict["asked"].as_array().unwrap();
    assert_eq!(
        pairs
            .iter()
            .map(|pair| pair["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["label", "prompt_visible"]
    );
    for pair in pairs {
        assert!(pair["question"]
            .as_str()
            .unwrap()
            .contains("Fixture worker"));
    }
    assert!(!serde_json::to_string(&feed)
        .unwrap()
        .contains(private_screen));
}
