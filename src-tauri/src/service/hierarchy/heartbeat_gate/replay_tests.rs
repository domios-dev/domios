use super::*;
use codeterm_types::decision::{DecisionPrimitives, DecisionRequest, TokenMode};

#[derive(Deserialize)]
struct Replay {
    plan_id: String,
    ticks: Vec<Tick>,
}

#[derive(Deserialize)]
struct Tick {
    at: u64,
    snapshot: Snapshot,
    incidents: Vec<String>,
}

/// Answers every question with one label; the tick label is the arm's verdict.
struct FixtureModel(&'static str);

impl DecisionModel for FixtureModel {
    fn id(&self) -> &str {
        "replay-fixture"
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
        panic!("multi-question request required")
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        Ok(DecisionResponse {
            usage: None,
            answers: request
                .questions
                .keys()
                .map(|key| {
                    let label = if key == "tick" {
                        self.0
                    } else if key.starts_with("learning:") {
                        "irrelevant"
                    } else {
                        "on_track"
                    };
                    (
                        key.clone(),
                        DecisionAnswer::Choice {
                            choice: label.into(),
                            probabilities: BTreeMap::from([(label.into(), 0.99)]),
                            confidence: Some(0.99),
                        },
                    )
                })
                .collect(),
        })
    }
}

/// Plan projection, worker health and learnings: the state an accepted reminder
/// leaves unchanged when it only moves the delta baseline.
fn absolute_state(snapshot: &Snapshot) -> Value {
    let health: Vec<_> = snapshot.workers.iter().map(Worker::health).collect();
    json!({ "projection": snapshot.projection, "health": health, "learnings": snapshot.learnings })
}

/// The policy oracle is independent of the gate's verdict: only an approving
/// adapter emits, once per new observation; hard incidents never force a wake.
fn run(arm: &str, model: Option<&dyn DecisionModel>, approving: bool) {
    let source = std::env::var("CODETERM_HEARTBEAT_REPLAY_INPUT")
        .ok()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../evals/heartbeat-replay/reconstructed.json"
            ))
            .into()
        });
    let replay: Replay = serde_json::from_str(&source).unwrap();
    assert!(!replay.ticks.is_empty());
    let gate = Gate::default();
    let (mut woken, mut suppressed) = (0, 0);
    let (mut missed, mut unexpected) = (Vec::new(), Vec::new());
    let mut decided: Option<(String, Value, Vec<HardWake>)> = None;
    for tick in &replay.ticks {
        let snapshot = &tick.snapshot;
        let identity = snapshot.hash();
        let state = absolute_state(snapshot);
        let same = decided.as_ref().is_some_and(|(id, prior, hard)| {
            *id == identity
                || (snapshot.delta.is_empty()
                    && *prior == state
                    && snapshot.hard.iter().all(|event| hard.contains(event)))
        });
        let expected = approving && !same;
        if !same {
            decided = Some((identity, state, snapshot.hard.clone()));
        }
        let decision = gate
            .decide(
                "owner",
                &tick.snapshot,
                tick.at,
                model,
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        if decision.wake {
            woken += 1;
            assert!(!decision.next_steps.is_empty());
            gate.delivered("owner", &tick.snapshot, &decision, tick.at);
        } else {
            suppressed += 1;
        }
        if expected && !decision.wake && !tick.incidents.is_empty() {
            missed.push(json!({"at": tick.at, "incidents": tick.incidents}));
        }
        if decision.wake && !expected {
            unexpected.push(json!({"at": tick.at, "reason": decision.reason}));
        }
        println!(
            "HEARTBEAT_REPLAY_TICK={}",
            json!({"at": tick.at, "wake": decision.wake, "expected": expected, "reason": decision.reason, "observation_id": decision.observation_id, "incidents": tick.incidents})
        );
    }
    println!(
        "HEARTBEAT_REPLAY_RESULT={}",
        json!({"plan_id": replay.plan_id, "arm": arm, "policy": "choice_approval_required", "ticks": replay.ticks.len(), "woken": woken, "suppressed": suppressed, "missed_incidents": missed.len(), "misses": missed, "unexpected_wakes": unexpected.len(), "unexpected": unexpected})
    );
    assert!(
        missed.is_empty(),
        "an approved new observation did not wake"
    );
    assert!(
        unexpected.is_empty(),
        "the gate woke without a new approval"
    );
}

#[test]
fn heartbeat_gate_replay_fixture() {
    run("fixture", Some(&FixtureModel("action_needed")), true);
}

#[test]
fn heartbeat_gate_replay_no_model() {
    run("no-model", None, false);
}
