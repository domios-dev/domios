use super::*;

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

/// Plan projection, worker health and learnings: the state an accepted reminder
/// leaves unchanged when it only moves the delta baseline.
fn absolute_state(snapshot: &Snapshot) -> Value {
    let health: Vec<_> = snapshot.workers.iter().map(Worker::health).collect();
    json!({ "projection": snapshot.projection, "health": health, "learnings": snapshot.learnings })
}

fn run() {
    let source = std::env::var("CODETERM_HEARTBEAT_REPLAY_INPUT")
        .ok()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../evals/heartbeat-replay/reconstructed.json"
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
        let has_step = !snapshot.hard.is_empty()
            || snapshot.workers.iter().any(Worker::unhealthy)
            || !snapshot.actionable_tasks.is_empty()
            || snapshot
                .delta
                .keys()
                .any(|key| key.starts_with("task:") || key.starts_with("report:"));
        let expected = !same && !snapshot.healthy_and_unchanged() && has_step;
        if !same {
            decided = Some((identity, state, snapshot.hard.clone()));
        }
        let decision = gate
            .decide_with_interval("owner", snapshot, tick.at, 0)
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
        json!({"plan_id": replay.plan_id, "arm": "rule", "policy": "concrete_next_step", "ticks": replay.ticks.len(), "woken": woken, "suppressed": suppressed, "missed_incidents": missed.len(), "misses": missed, "unexpected_wakes": unexpected.len(), "unexpected": unexpected})
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
    run();
}
