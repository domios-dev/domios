use super::*;

#[test]
fn unchanged_worker_frames_call_the_model_once() {
    for answer in [
        Ok(choice(READY_IDLE, 0.99)),
        Err(DecisionError::Unsupported),
    ] {
        let (host, calls, _) = StubHost::new("unchanged-frame", StubModel::answering(answer));
        let tab_id = tab();
        run(&host, &tab_id, ObservationTrigger::WorkerIdle);
        for _ in 0..100 {
            state::note_output(&tab_id);
            assert!(run(&host, &tab_id, ObservationTrigger::WorkerIdle).is_none());
            assert!(run(&host, &tab_id, ObservationTrigger::BusyNoOutput).is_none());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn supporter_tab_state_other_label_takes_no_action() {
    let model = StubModel::answering(Ok(choice(OTHER, 0.99)));
    let (host, calls, _) = StubHost::new("ambiguous fixture", model);
    let tab_id = tab();
    let verdict = run(&host, &tab_id, ObservationTrigger::WorkerIdle).unwrap();
    assert_eq!(verdict.label, OTHER);
    assert_eq!(verdict.action, SignalAction::None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(host.deliveries.lock().is_empty());
    let status = host.runtime.status(600);
    assert_eq!(status["last_verdict"]["tab_state"]["label"], OTHER);
}

#[test]
fn supporter_tab_state_budget_skips_model_call_and_signal() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let (host, calls, _) = StubHost::new("healthy fixture", model);
    host.runtime
        .reserve(1, VerdictKind::Rerank, None, 1)
        .unwrap();
    let config = SupporterConfig {
        calls_per_hour: Some(1),
        ..enabled()
    };
    let verdict = spawn_observer(
        &host,
        &config,
        "fixture",
        3000,
        &tab(),
        ObservationTrigger::SpawnReady,
        "structural",
    )
    .unwrap()
    .join()
    .unwrap();
    assert!(verdict.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(host.deliveries.lock().is_empty());
}

#[test]
fn tab_state_single_request_label_and_prompt_visible() {
    let model = StubModel::answering(Ok(choice(WORKING_NORMALLY, 0.91)));
    let (host, calls, seen) = StubHost::new("opaque shared screen", model);
    let verdict = run(&host, &tab(), ObservationTrigger::WorkerIdle).unwrap();
    let request = seen.lock().clone().unwrap();
    assert_eq!(request.questions.len(), 2);
    assert!(matches!(
        &request.questions["label"].question,
        codeterm_types::decision::DecisionQuestion::Choice { .. }
    ));
    assert!(matches!(
        &request.questions["prompt_visible"].question,
        codeterm_types::decision::DecisionQuestion::Noul { .. }
    ));
    assert_eq!(request.state["screen"], "opaque shared screen");
    assert_eq!(verdict.prompt_visible, 0.0);
    assert_eq!(verdict.safe_choice, None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.runtime.status(600)["calls_last_hour"], 1);
    let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    let line: Value = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["action"] == "verdict")
        .unwrap();
    assert_eq!(line["question_count"], 2);
    assert!(line["distribution"]["label"]["probabilities"].is_object());
    assert_eq!(line["distribution"]["prompt_visible"]["p"], 0.0);
    assert!(!text.contains("opaque shared screen"));
}

#[test]
fn tab_state_extra_safe_choice_is_carried_once_and_parsed_from_answer_map() {
    let model = StubModel::answering(Ok(choice(WORKING_NORMALLY, 0.91)));
    let (host, _, _) = StubHost::new("opaque", model);
    let (_, metadata) =
        prepare_observation(&host, &tab(), ObservationTrigger::WorkerIdle, "opaque").unwrap();
    let extra = codeterm_types::decision::DecisionQuestion::Choice {
        options: BTreeMap::from([
            ("safe-provider-value".into(), "opaque-safe".into()),
            ("abstain".into(), "opaque-abstain".into()),
        ]),
    };
    let questions = questions::observation_questions(&metadata, Some(extra.clone()));
    assert_eq!(questions.len(), 3);
    assert_eq!(questions["safe_choice"].question, extra);
    let response = DecisionResponse {
        answers: BTreeMap::from([
            ("label".into(), choice(TRUST_OR_PERMISSION_PROMPT, 0.95)),
            ("prompt_visible".into(), DecisionAnswer::Noul { p: 0.99 }),
            ("safe_choice".into(), choice("safe-provider-value", 0.98)),
        ]),
        usage: None,
    };
    let verdict = observation_answers(response).unwrap();
    assert_eq!(verdict.confidence, 0.95);
    assert_eq!(verdict.prompt_visible, 0.99);
    assert_eq!(
        verdict.safe_choice,
        Some(("safe-provider-value".into(), 0.98))
    );
}
