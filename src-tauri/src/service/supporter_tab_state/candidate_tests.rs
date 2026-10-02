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
            identity::DecisionSource::Error,
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
        identity::DecisionSource::Error
    );
}

#[test]
fn supporter_first_opinion_scrape_no_adapter_identical_zero_work() {
    let (host, calls, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    let callback = Arc::new(AtomicUsize::new(0));
    let called = callback.clone();
    assert!(
        candidates::spawn(&host, &SupporterConfig::default(), &tab(), move |_, _| {
            called.fetch_add(1, Ordering::SeqCst);
        })
        .is_none()
    );
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
        adapter: "fixture".into(),
        ..Default::default()
    };
    let id = tab();
    candidates::spawn(&host, &config, &id, move |verdict, source| {
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
    candidates::spawn(&host, &config, &tab(), |_, _| {
        panic!("disconnected verdict must be discarded")
    })
    .unwrap()
    .join()
    .unwrap();
}

fn prompt() -> PendingPrompt {
    PendingPrompt {
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

#[test]
fn supporter_first_opinion_scrape_a004b77d4_transcript_and_composer_copy_negatives() {
    let live = "Fixture selection\n❯ 1. Opaque alpha\n2. Opaque beta\nEnter to confirm\n";
    let transcript = format!("{live}Selected alpha\nCompleted response\n❯\n");
    let draft = "❯ Fixture selection\n1. Opaque alpha\n2. Opaque beta\nEnter to confirm\n";
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
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
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
        .publish_model_scrape_candidate(&snapshot, &verdict);
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
        .publish_model_scrape_candidate(&snapshot, &verdict);
    let tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    assert_eq!(tab.pending_prompt, Some(prompt()));
    assert_eq!(tab.pending_prompt_kind, PromptKind::Provider);
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&id),
        0
    );
}
