use super::*;
use codeterm_types::decision::{DecisionPrimitives, DecisionRequest, TokenMode};
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
        Self { calls: AtomicUsize::new(0), tick: "nothing_changed", worker: "on_track", confidence: 0.99, error: None }
    }
}

impl DecisionModel for FixtureModel {
    fn id(&self) -> &str { "fixture" }
    fn token_mode(&self) -> TokenMode { TokenMode::None }
    fn primitives(&self) -> DecisionPrimitives { DecisionPrimitives { choice: true, ..Default::default() } }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        panic!("the gate must submit one multi-question request")
    }
    fn decide_questions(&self, request: &DecisionQuestionsRequest, _: Instant) -> Result<DecisionResponse, DecisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.error { return Err(error.clone()); }
        Ok(DecisionResponse { usage: None, answers: request.questions.iter().map(|(key, _)| {
            let choice = if key == "tick" { self.tick } else if key.starts_with("learning:") { "irrelevant" } else { self.worker };
            (key.clone(), DecisionAnswer::Choice { choice: choice.into(), probabilities: BTreeMap::from([(choice.into(), self.confidence)]), confidence: Some(self.confidence) })
        }).collect() })
    }
}

fn healthy_snapshot() -> Snapshot {
    Snapshot { plan_id: "plan".into(), workers: vec![Worker {
        task_id: "task".into(), tab_id: "worker".into(), ack_received: true,
        session_key: Some("own-session:nonce".into()), process: "present".into(),
        ..Default::default()
    }], ..Default::default() }
}

fn tick(gate: &Gate, snapshot: &Snapshot, now: u64, model: Option<&dyn DecisionModel>) -> Decision {
    gate.decide("owner", snapshot, now, model, Instant::now() + Duration::from_secs(1)).unwrap()
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
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn heartbeat_gate_new_report_wakes() {
    let mut snapshot = healthy_snapshot();
    snapshot.delta.insert("report:r1".into(), "persisted".into());
    let model = FixtureModel { tick: "action_needed", ..FixtureModel::healthy() };
    assert!(tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_undelivered_report_wakes_despite_nothing_changed() {
    let mut snapshot = healthy_snapshot();
    snapshot.hard.push(HardWake::UndeliveredReport("r1".into()));
    let model = FixtureModel::healthy();
    let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
    assert!(decision.wake);
    assert!(!decision.next_steps.is_empty());
}

#[test]
fn heartbeat_gate_codex_update_prompt_wakes_with_waiting_on_prompt_next_step() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].prompt = Some("manual_update".into());
    snapshot.workers[0].safe_choice = Some("fixture-option".into());
    let model = FixtureModel { worker: "waiting_on_prompt", ..FixtureModel::healthy() };
    for model in [Some(&model as &dyn DecisionModel), None] {
        let decision = tick(&Gate::default(), &snapshot, 100, model);
        assert!(decision.wake);
        assert_eq!(decision.worker_labels["worker"], "waiting_on_prompt");
        assert!(decision.next_steps.iter().any(|step| step.contains("fixture-option")));
    }
}

#[test]
fn heartbeat_gate_idle_without_report_past_threshold_wakes() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].idle_overdue = true;
    let model = FixtureModel { worker: "idle_no_report", ..FixtureModel::healthy() };
    assert!(tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_looping_wakes() {
    let model = FixtureModel { worker: "looping", ..FixtureModel::healthy() };
    assert!(tick(&Gate::default(), &healthy_snapshot(), 100, Some(&model)).wake);
}

#[test]
fn heartbeat_gate_review_hold_not_flagged() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].held = true;
    snapshot.workers[0].idle_overdue = true;
    let model = FixtureModel { worker: "idle_no_report", ..FixtureModel::healthy() };
    assert!(!tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    assert!(!tick(&Gate::default(), &snapshot, 100, None).wake);
}

#[test]
fn heartbeat_gate_other_or_low_confidence_wakes() {
    for (tick_label, worker, confidence) in [("other", "on_track", 0.99), ("nothing_changed", "other", 0.99), ("nothing_changed", "on_track", 0.79)] {
        let model = FixtureModel { tick: tick_label, worker, confidence, ..FixtureModel::healthy() };
        assert!(tick(&Gate::default(), &healthy_snapshot(), 100, Some(&model)).wake);
    }
}

#[test]
fn heartbeat_gate_budget_exhausted_uses_fallback() {
    let model = FixtureModel { error: Some(DecisionError::Budget), ..FixtureModel::healthy() };
    let mut snapshot = healthy_snapshot();
    assert!(!tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
    snapshot.delta.insert("stride".into(), "new".into());
    let decision = tick(&Gate::default(), &snapshot, 100, Some(&model));
    assert!(decision.wake);
    assert_eq!(decision.decided_by, "fallback(budget_exhausted)");
}

#[test]
fn heartbeat_gate_safety_line_after_long_silence() {
    let gate = Gate::default();
    let model = FixtureModel::healthy();
    let snapshot = healthy_snapshot();
    tick(&gate, &snapshot, 100, Some(&model));
    let decision = tick(&gate, &snapshot, 100 + SAFETY_STATUS_SECS, Some(&model));
    assert!(decision.wake && decision.safety);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    gate.delivered("owner", &snapshot, &decision, 100 + SAFETY_STATUS_SECS);
    assert!(!tick(&gate, &snapshot, 101 + SAFETY_STATUS_SECS, Some(&model)).wake);
}

#[test]
fn heartbeat_gate_hard_wakes_not_suppressible() {
    let model = FixtureModel::healthy();
    for hard in [HardWake::UndeliveredReport("r".into()), HardWake::FailedGate("g".into()), HardWake::OwnerMessage("m".into()), HardWake::ChoiceAnswer("c".into()), HardWake::Incident("i".into()), HardWake::ModeChange] {
        let mut snapshot = healthy_snapshot();
        snapshot.hard.push(hard);
        assert!(tick(&Gate::default(), &snapshot, 100, Some(&model)).wake);
        assert!(tick(&Gate::default(), &snapshot, 100, None).wake);
    }
}

#[test]
fn heartbeat_gate_learnings_capped_at_three() {
    let mut snapshot = healthy_snapshot();
    snapshot.learnings = (0..8).map(|i| (format!("l{i}"), format!("learning {i}"))).collect();
    let request = questions(&snapshot);
    assert_eq!(request.questions.keys().filter(|key| key.starts_with("learning:")).count(), 8);
    let choices: BTreeMap<_, _> = request.questions.keys().map(|key| (key.clone(), (if key.starts_with("learning:") { "relevant" } else { "on_track" }.to_string(), 0.99))).collect();
    assert_eq!(selected_learnings(&choices).len(), 3);
}

#[test]
fn heartbeat_gate_journal_decided_by_and_observation_id() {
    let decision = tick(&Gate::default(), &healthy_snapshot(), 100, None);
    let event = decision.journal_event("owner", "plan", 100);
    assert_eq!(event["kind"], "heartbeat");
    assert_eq!(event["decided_by"], "fallback(no_adapter)");
    assert!(event["observation_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(event.get("snapshot").is_none());
}

#[test]
fn heartbeat_gate_failed_delivery_reuses_verdict_and_pending_report_bypasses_hash() {
    let gate = Gate::default();
    let mut snapshot = healthy_snapshot();
    snapshot.hard.push(HardWake::UndeliveredReport("r".into()));
    let model = FixtureModel::healthy();
    let decision = tick(&gate, &snapshot, 100, Some(&model));
    assert!(tick(&gate, &snapshot, 101, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    gate.delivered("owner", &snapshot, &decision, 101);
    assert!(tick(&gate, &snapshot, 102, Some(&model)).wake);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}
