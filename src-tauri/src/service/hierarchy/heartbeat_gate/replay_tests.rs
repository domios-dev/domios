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

struct FixtureModel;

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
                        "nothing_changed"
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

fn run(arm: &str, model: Option<&dyn DecisionModel>) {
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
    let mut woken = 0;
    let mut suppressed = 0;
    let mut missed = Vec::new();
    for tick in &replay.ticks {
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
            assert!(!decision.next_steps.is_empty() || decision.safety);
            gate.delivered("owner", &tick.snapshot, &decision, tick.at);
        } else {
            suppressed += 1;
            if !tick.incidents.is_empty() {
                missed.push(json!({"at": tick.at, "incidents": tick.incidents}));
            }
        }
        println!(
            "HEARTBEAT_REPLAY_TICK={}",
            json!({"at": tick.at, "wake": decision.wake, "reason": decision.reason, "observation_id": decision.observation_id, "incidents": tick.incidents})
        );
    }
    println!(
        "HEARTBEAT_REPLAY_RESULT={}",
        json!({"plan_id": replay.plan_id, "arm": arm, "ticks": replay.ticks.len(), "woken": woken, "suppressed": suppressed, "missed_incidents": missed.len(), "misses": missed})
    );
    assert!(
        missed.is_empty(),
        "the production gate missed a labeled incident"
    );
}

#[test]
fn heartbeat_gate_replay_fixture() {
    run("fixture", Some(&FixtureModel));
}

#[test]
fn heartbeat_gate_replay_no_model() {
    run("no-model", None);
}
