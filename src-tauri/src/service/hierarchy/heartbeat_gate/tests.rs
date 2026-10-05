use super::*;
use codeterm_types::decision::{deadline, DecisionPrimitives, DecisionRequest, TokenMode};
use std::sync::atomic::{AtomicUsize, Ordering};

struct FixtureModel {
    calls: AtomicUsize,
    tick: &'static str,
    worker: &'static str,
    confidence: f64,
    error: Option<DecisionError>,
}

impl FixtureModel {
    fn healthy() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            tick: "nothing_changed",
            worker: "on_track",
            confidence: 0.99,
            error: None,
        }
    }

    fn approving() -> Self {
        Self {
            tick: "action_needed",
            ..Self::healthy()
        }
    }
}

impl DecisionModel for FixtureModel {
    fn id(&self) -> &str {
        "fixture"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            ..Default::default()
        }
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        panic!("the gate must submit one multi-question request")
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        codeterm_types::decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(DecisionResponse {
            usage: None,
            answers: request
                .questions
                .keys()
                .map(|key| {
                    let choice = if key == "tick" {
                        self.tick
                    } else if key.starts_with("learning:") {
                        "irrelevant"
                    } else {
                        self.worker
                    };
                    (
                        key.clone(),
                        DecisionAnswer::Choice {
                            choice: choice.into(),
                            probabilities: BTreeMap::from([(choice.into(), self.confidence)]),
                            confidence: Some(self.confidence),
                        },
                    )
                })
                .collect(),
        })
    }
}

fn healthy_snapshot() -> Snapshot {
    Snapshot {
        plan_id: "plan".into(),
        workers: vec![Worker {
            task_id: "task".into(),
            tab_id: "worker".into(),
            ack_received: true,
            session_key: Some("own-session:nonce".into()),
            process: "present".into(),
            activity: "working".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn tick(gate: &Gate, snapshot: &Snapshot, now: u64, model: Option<&dyn DecisionModel>) -> Decision {
    gate.decide(
        "owner",
        snapshot,
        now,
        model,
        deadline::now() + Duration::from_secs(1),
    )
    .unwrap()
}

#[test]
fn heartbeat_gate_on_track_no_delta_no_message() {
    let gate = Gate::default();
    let model = FixtureModel::healthy();
    assert!(!tick(&gate, &healthy_snapshot(), 100, Some(&model)).wake);
    assert!(!tick(&Gate::default(), &healthy_snapshot(), 100, None).wake);
}

#[test]
fn heartbeat_gate_identical_snapshot_no_model_call() {
    let gate = Gate::default();
    let model = FixtureModel::healthy();
    let snapshot = healthy_snapshot();
    assert!(!tick(&gate, &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&gate, &snapshot, 101, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn heartbeat_gate_new_report_wakes() {
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("report:r1".into(), "persisted".into());
    let model = FixtureModel::approving();
    assert!(tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_undelivered_report_wakes_only_on_approval() {
    let mut snapshot = healthy_snapshot();
    let hard = HardWake::UndeliveredReport("r1".into());
    snapshot.hard.push(hard.clone());
    let denial = FixtureModel::healthy();
    assert!(!tick(&Gate::default(), &snapshot, 100, Some(&denial)).wake);
    let decision = tick(
        &Gate::default(),
        &snapshot,
        100,
        Some(&FixtureModel::approving()),
    );
    assert!(decision.wake);
    assert_eq!(decision.next_steps[0], hard.next_step(&snapshot.plan_id));
}

#[test]
fn heartbeat_gate_codex_update_prompt_wakes_with_waiting_on_prompt_next_step() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].prompt = Some("manual_update".into());
    snapshot.workers[0].safe_choice = Some("fixture-option".into());
    let model = FixtureModel {
        worker: "waiting_on_prompt",
        ..FixtureModel::approving()
    };
    let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
    assert!(decision.wake);
    assert_eq!(decision.worker_labels["worker"], "waiting_on_prompt");
    assert!(decision
        .next_steps
        .iter()
        .any(|step| step.contains("fixture-option")));
    assert!(!tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_idle_without_report_past_threshold_wakes() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].idle_overdue = true;
    let model = FixtureModel {
        worker: "idle_no_report",
        ..FixtureModel::approving()
    };
    assert!(tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_looping_wakes() {
    let model = FixtureModel {
        worker: "looping",
        ..FixtureModel::approving()
    };
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].repeating = true;
    assert!(tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_review_hold_not_flagged() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].held = true;
    snapshot.workers[0].idle_overdue = true;
    let model = FixtureModel {
        worker: "idle_no_report",
        ..FixtureModel::healthy()
    };
    assert!(!tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_other_or_low_confidence_suppresses() {
    for (tick_label, worker, confidence) in [
        ("other", "on_track", 0.99),
        ("nothing_changed", "other", 0.99),
        ("nothing_changed", "on_track", 0.79),
    ] {
        let model = FixtureModel {
            tick: tick_label,
            worker,
            confidence,
            ..FixtureModel::healthy()
        };
        let mut snapshot = healthy_snapshot();
        snapshot
            .delta
            .insert("stride:fixture".into(), "opaque evidence".into());
        let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
        assert!(!decision.wake);
        assert_eq!(decision.reason, "low_confidence");
    }
}

#[test]
fn heartbeat_gate_blocked_is_declared_and_never_wakes() {
    let request = questions(&healthy_snapshot());
    let options = match &request.questions["tick"].question {
        codeterm_types::decision::DecisionQuestion::Choice { options } => options,
        _ => panic!("tick must remain a choice"),
    };
    assert!(options.contains_key("blocked"));

    let model = FixtureModel {
        tick: "blocked",
        ..FixtureModel::healthy()
    };
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("task:fixture".into(), "opaque change".into());
    let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
    assert_eq!(decision.tick_label, "blocked");
    assert!(!decision.wake);
}

#[test]
fn heartbeat_gate_budget_exhausted_suppresses_without_fallback() {
    let model = FixtureModel {
        error: Some(DecisionError::Budget),
        ..FixtureModel::approving()
    };
    let mut snapshot = healthy_snapshot();
    assert!(!tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    snapshot.delta.insert("stride".into(), "new".into());
    let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
    assert!(!decision.wake);
    assert_eq!(decision.reason, "budget_exhausted");
    assert_eq!(decision.decided_by, "adapter_error");
}

#[test]
fn heartbeat_gate_learnings_capped_at_three() {
    let mut snapshot = healthy_snapshot();
    snapshot.learnings = (0..8)
        .map(|i| (format!("l{i}"), format!("learning {i}")))
        .collect();
    let request = questions(&snapshot);
    assert_eq!(
        request
            .questions
            .keys()
            .filter(|key| key.starts_with("learning:"))
            .count(),
        8
    );
    let choices: BTreeMap<_, _> = request
        .questions
        .keys()
        .map(|key| {
            (
                key.clone(),
                (
                    if key.starts_with("learning:") {
                        "relevant"
                    } else {
                        "on_track"
                    }
                    .to_string(),
                    0.99,
                ),
            )
        })
        .collect();
    assert_eq!(selected_learnings(&choices).len(), 3);
}

#[test]
fn heartbeat_gate_journal_decided_by_and_observation_id() {
    let decision = tick(&Gate::default(), &healthy_snapshot(), 100, None);
    let event = decision.journal_event("owner", "plan", 100);
    assert_eq!(event["kind"], "heartbeat");
    assert_eq!(event["decided_by"], "no_adapter");
    assert_eq!(event["action"], "suppress");
    assert!(event["observation_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));
    for key in ["snapshot", "observation", "next_steps"] {
        assert!(event.get(key).is_none());
    }
}

#[test]
fn heartbeat_gate_failed_delivery_never_replays_unchanged_observation() {
    let gate = Gate::default();
    let mut snapshot = healthy_snapshot();
    snapshot.hard.push(HardWake::UndeliveredReport("r".into()));
    let model = FixtureModel::approving();
    let decision = tick(&gate, &snapshot, 100, Some(&model));
    assert!(decision.wake);
    assert!(!tick(&gate, &snapshot, 101, Some(&model)).wake);
    gate.delivered("owner", &snapshot, &decision, 101);
    assert!(!tick(&gate, &snapshot, 102, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn heartbeat_gate_budget_and_redacted_feed_share_runtime() {
    let clock = deadline::FixtureClock::new(Instant::now() - Duration::from_secs(60));
    let _scope = clock.enter();
    let dir = tempfile::tempdir().unwrap();
    let runtime = std::sync::Arc::new(crate::service::supporter::SupporterRuntime::new(
        dir.path().into(),
    ));
    let fixture = std::sync::Arc::new(FixtureModel::healthy());
    let model = crate::service::supporter::budgeted_model_with_activity(
        fixture.clone(),
        runtime.clone(),
        1,
        crate::service::supporter::VerdictKind::Heartbeat,
        Some("private-tab".into()),
        None,
        None,
    );
    let gate = Gate::default();
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("stride:fixture".into(), "opaque evidence".into());
    let decision = tick(&gate, &snapshot, 100, Some(model.as_ref()));
    runtime
        .heartbeat_event(decision.journal_event("private-tab", "private-plan", 100))
        .unwrap();
    let private = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert_eq!(
        private
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|row| row["kind"] == "model_call")
            .count(),
        1
    );
    let recorded: Value = private
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|row| row["observation_id"] == decision.observation_id)
        .unwrap();
    assert_eq!(recorded["state_hash"], snapshot.hash());
    assert!(recorded.get("observation").is_none());
    assert!(!private.contains(snapshot.workers[0].session_key.as_deref().unwrap()));
    assert_eq!(recorded["tick_class"], decision.tick_label);
    assert_eq!(runtime.status(1)["calls_last_hour"], 1);
    let mut next = snapshot.clone();
    next.delta
        .insert("private-stride".into(), "private-evidence".into());
    let exhausted = tick(&gate, &next, 101, Some(model.as_ref()));
    assert_eq!(exhausted.reason, "budget_exhausted");
    assert!(!exhausted.wake);
    let mut bounded = next.clone();
    bounded.truncated = true;
    let overflow = tick(&gate, &bounded, 102, Some(model.as_ref()));
    assert_eq!(overflow.reason, "observation_bounds");
    runtime
        .heartbeat_event(overflow.journal_event("private-tab", "private-plan", 102))
        .unwrap();
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    let feed = runtime.feed(100, None).unwrap();
    assert!(feed
        .iter()
        .any(|row| row["kind"] == "heartbeat" && row["action"] == "suppress"));
    assert!(feed
        .iter()
        .any(|row| row["reason"] == "observation_bounds" && row["fallback"] == false));
    for row in feed {
        assert!(row.get("observation").is_none());
        assert!(row.get("observation_id").is_none());
        assert!(row.get("decided_by").is_none());
        assert!(row.get("tab").is_none());
    }
}

#[test]
fn heartbeat_gate_projection_advances_only_after_delivery() {
    let gate = Gate::default();
    let key = "owner";
    gate.observed(
        key,
        Observation {
            projection: Some(BTreeMap::from([("mode".into(), "execution".into())])),
            ..Default::default()
        },
    );
    let mut snapshot = healthy_snapshot();
    snapshot
        .projection
        .insert("mode".into(), "execution".into());
    snapshot
        .projection
        .insert("report:r".into(), "digest".into());
    snapshot.delta.insert("report:r".into(), "digest".into());
    let decision = tick(&gate, &snapshot, 100, Some(&FixtureModel::approving()));
    assert!(decision.wake);
    assert!(!gate
        .observation(key)
        .projection
        .unwrap()
        .contains_key("report:r"));
    gate.delivered(key, &snapshot, &decision, 100);
    assert!(gate
        .observation(key)
        .projection
        .unwrap()
        .contains_key("report:r"));
}

#[test]
fn heartbeat_gate_review_hold_does_not_hide_missing_session_or_prompt() {
    let model = FixtureModel::approving();
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].held = true;
    snapshot.workers[0].session_key = None;
    for prompt in [None, Some("provider".to_string())] {
        snapshot.workers[0].prompt = prompt;
        let worker = &snapshot.workers[0];
        let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
        assert!(decision.wake);
        assert_eq!(decision.worker_labels["worker"], worker.heuristic_label());
        assert!(decision
            .next_steps
            .contains(&worker.next_step(worker.heuristic_label())));
    }
}

#[test]
fn heartbeat_gate_working_three_ticks_no_calls_prompt_fourth_wakes_once() {
    let gate = Gate::default();
    let model = FixtureModel::approving();
    let mut snapshot = healthy_snapshot();
    let initial = snapshot.hash();
    for now in 100..103 {
        snapshot.workers[0].appended_turns = vec![format!("turn-{now}")];
        snapshot.workers[0].last_turn = Some(format!("turn-{now}"));
        snapshot.workers[0].turn_cursor = Some(format!("offset-{now}"));
        snapshot.workers[0].elapsed_bucket = now;
        snapshot.workers[0].lane_commits = vec![format!("commit-{now}")];
        snapshot.workers[0].recent_turn_hashes = vec![format!("content-{now}")];
        assert_eq!(snapshot.hash(), initial);
        assert!(!tick(&gate, &snapshot, now, Some(&model)).wake);
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    snapshot.workers[0].prompt = Some("manual_update".into());
    let decision = tick(&gate, &snapshot, 103, Some(&model));
    assert!(decision.wake);
    assert_eq!(decision.worker_labels["worker"], "waiting_on_prompt");
    gate.delivered("owner", &snapshot, &decision, 103);
    snapshot.workers[0].turn_cursor = Some("offset-new".into());
    assert!(!tick(&gate, &snapshot, 104, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn heartbeat_gate_soft_reason_cooldown_allows_new_class_and_hard_wakes() {
    let gate = Gate::default();
    let model = FixtureModel {
        tick: "action_needed",
        ..FixtureModel::healthy()
    };
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("stride:a".into(), "opaque first".into());
    let first = tick(&gate, &snapshot, 100, Some(&model));
    assert!(first.wake);
    gate.delivered("owner", &snapshot, &first, 100);
    snapshot
        .delta
        .insert("stride:b".into(), "opaque second".into());
    assert!(!tick(&gate, &snapshot, 101, Some(&model)).wake);
    snapshot.workers[0].prompt = Some("manual_update".into());
    let prompt = tick(&gate, &snapshot, 102, Some(&model));
    assert!(prompt.wake);
    gate.delivered("owner", &snapshot, &prompt, 102);
    snapshot
        .hard
        .push(HardWake::UndeliveredReport("report".into()));
    assert!(tick(&gate, &snapshot, 103, Some(&model)).wake);
}

#[test]
fn heartbeat_gate_observed_mode_change_is_detected_until_delivered() {
    let gate = Gate::default();
    gate.observed(
        "owner",
        Observation {
            projection: Some(BTreeMap::from([("mode".into(), "Execution:Active".into())])),
            ..Default::default()
        },
    );
    assert!(!gate.mode_changed("owner", "Execution:Active"));
    assert!(gate.mode_changed("owner", "Paused:Active"));
    let mut snapshot = healthy_snapshot();
    snapshot
        .projection
        .insert("mode".into(), "Paused:Active".into());
    snapshot.hard.push(HardWake::ModeChange);
    let decision = tick(&gate, &snapshot, 100, Some(&FixtureModel::approving()));
    assert!(decision.wake);
    gate.delivered("owner", &snapshot, &decision, 100);
    assert!(!gate.mode_changed("owner", "Paused:Active"));
}

#[test]
fn heartbeat_fixture_clock_preserves_budget_and_suppress_after_persistence() {
    let clock = deadline::FixtureClock::new(Instant::now() - Duration::from_secs(60));
    let origin = clock.now();
    let _scope = clock.enter();
    assert!(origin + Duration::from_secs(1) < Instant::now());
    let dir = tempfile::tempdir().unwrap();
    let runtime = std::sync::Arc::new(crate::service::supporter::SupporterRuntime::new(
        dir.path().into(),
    ));
    let events = std::sync::Arc::new(AtomicUsize::new(0));
    let persisted = events.clone();
    let during_persistence = clock.clone();
    runtime.attach_visibility(
        std::sync::Arc::new(|_| None),
        std::sync::Arc::new(move |_| {
            persisted.fetch_add(1, Ordering::SeqCst);
            during_persistence.advance_to(origin + Duration::from_millis(500));
        }),
    );
    let fixture = std::sync::Arc::new(FixtureModel::healthy());
    let model = crate::service::supporter::budgeted_model_with_activity(
        fixture.clone(),
        runtime.clone(),
        1,
        crate::service::supporter::VerdictKind::Heartbeat,
        Some("private-tab".into()),
        None,
        None,
    );
    let gate = Gate::default();
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("stride:fixture".into(), "opaque evidence".into());
    let decision = tick(&gate, &snapshot, 100, Some(model.as_ref()));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(1)["calls_last_hour"], 1);
    assert_eq!(decision.decided_by, "adapter");
    assert!(!decision.wake);
    assert_eq!(decision.worker_labels["worker"], "on_track");
    runtime
        .heartbeat_event(decision.journal_event("private-tab", "private-plan", 100))
        .unwrap();
    snapshot
        .delta
        .insert("stride:next".into(), "private-evidence".into());
    let exhausted = tick(&gate, &snapshot, 101, Some(model.as_ref()));
    assert_eq!(exhausted.reason, "budget_exhausted");
    assert!(!exhausted.wake);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(1)["calls_last_hour"], 1);
    assert!(codeterm_types::decision::physical::current().is_none());
    let journal = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let rows: Vec<Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        rows.iter()
            .filter(|row| row["kind"] == "model_call")
            .count(),
        1
    );
    let request = rows
        .iter()
        .position(|row| row["kind"] == "model_call")
        .unwrap();
    let spend = rows
        .iter()
        .position(|row| row["kind"] == "spend_updated")
        .unwrap();
    let verdict = rows
        .iter()
        .position(|row| row["action"] == "verdict")
        .unwrap();
    let suppress = rows
        .iter()
        .position(|row| row["action"] == "suppress")
        .unwrap();
    assert!(request < spend && spend < verdict && verdict < suppress);
    assert!(rows[suppress].get("observation").is_none());
    assert!(!journal.contains("own-session:nonce"));
    assert_eq!(events.load(Ordering::SeqCst), rows.len());
    let feed = runtime.feed(100, None).unwrap();
    assert!(feed
        .iter()
        .any(|row| row["kind"] == "heartbeat" && row["action"] == "suppress"));
    for row in feed {
        for key in ["observation", "observation_id", "decided_by", "tab"] {
            assert!(row.get(key).is_none());
        }
    }
}

const DAY: u64 = 24 * 60 * 60;

fn every_hard_wake() -> Vec<HardWake> {
    vec![
        HardWake::UndeliveredReport("r".into()),
        HardWake::NewReport("r".into()),
        HardWake::TaskState("task".into()),
        HardWake::FailedGate("g".into()),
        HardWake::OwnerMessage("m".into()),
        HardWake::ChoiceAnswer("c".into()),
        HardWake::Incident("i".into()),
        HardWake::ModeChange,
    ]
}

fn evidence_snapshot(hard: HardWake) -> Snapshot {
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("report:r".into(), "opaque digest".into());
    snapshot.workers[0].prompt = Some("manual_update".into());
    snapshot.hard.push(hard);
    snapshot
}

#[test]
fn heartbeat_gate_unavailable_adapter_suppresses_hard_delta_and_silence() {
    for hard in every_hard_wake() {
        let gate = Gate::default();
        let snapshot = evidence_snapshot(hard.clone());
        assert!(!tick(&gate, &snapshot, 100, None).wake, "{hard:?}");
        assert!(!tick(&gate, &snapshot, 100 + DAY, None).wake, "{hard:?}");
    }
}

#[test]
fn heartbeat_gate_model_denial_or_uncertainty_beats_hard_and_silence() {
    let answers = [
        ("nothing_changed", "on_track", 0.99),
        ("status_only", "on_track", 0.99),
        ("other", "on_track", 0.99),
        ("action_needed", "on_track", 0.79),
        ("action_needed", "other", 0.99),
        ("action_needed", "on_track", f64::NAN),
    ];
    for (tick_label, worker, confidence) in answers {
        for hard in every_hard_wake() {
            let model = FixtureModel {
                tick: tick_label,
                worker,
                confidence,
                ..FixtureModel::healthy()
            };
            let gate = Gate::default();
            let snapshot = evidence_snapshot(hard.clone());
            let case = format!("{tick_label}/{worker}/{confidence}/{hard:?}");
            assert!(!tick(&gate, &snapshot, 100, Some(&model)).wake, "{case}");
            assert!(
                !tick(&gate, &snapshot, 100 + DAY, Some(&model)).wake,
                "{case}"
            );
            assert_eq!(model.calls.load(Ordering::SeqCst), 1, "{case}");
        }
    }
}

#[test]
fn heartbeat_gate_model_errors_suppress_without_fallback() {
    for error in [
        DecisionError::Budget,
        DecisionError::Deadline,
        DecisionError::CreditExhausted,
        DecisionError::Unavailable("opaque".into()),
        DecisionError::Transport("opaque".into()),
        DecisionError::Parse("opaque".into()),
    ] {
        let model = FixtureModel {
            error: Some(error.clone()),
            ..FixtureModel::healthy()
        };
        let gate = Gate::default();
        let snapshot = evidence_snapshot(HardWake::UndeliveredReport("r".into()));
        assert!(!tick(&gate, &snapshot, 100, Some(&model)).wake, "{error:?}");
        assert!(
            !tick(&gate, &snapshot, 100 + DAY, Some(&model)).wake,
            "{error:?}"
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 1, "{error:?}");
    }
}

#[test]
fn heartbeat_gate_elapsed_silence_alone_never_wakes() {
    let fixture = FixtureModel::healthy();
    for model in [Some(&fixture as &dyn DecisionModel), None] {
        let gate = Gate::default();
        let snapshot = healthy_snapshot();
        for now in [100, 100 + DAY, 100 + 2 * DAY] {
            assert!(!tick(&gate, &snapshot, now, model).wake);
        }
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn heartbeat_gate_valid_approval_wakes_once_per_unchanged_snapshot() {
    let model = FixtureModel {
        tick: "action_needed",
        ..FixtureModel::healthy()
    };
    let gate = Gate::default();
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("stride:a".into(), "opaque first".into());
    assert!(tick(&gate, &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&gate, &snapshot, 101, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    snapshot
        .delta
        .insert("stride:b".into(), "opaque second".into());
    assert!(tick(&gate, &snapshot, 102, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
}
