use super::*;

pub(crate) fn criteria_version() -> String {
    "heartbeat-rule-v1".into()
}

pub(crate) fn cadence(snapshot: &Snapshot) -> Decision {
    let mut decision = suppressed(snapshot, "rule", "actionable_rule");
    decision.wake = true;
    decision.tick_label = "action_needed".into();
    let worker_steps = snapshot
        .workers
        .iter()
        .filter_map(|worker| {
            let label = worker.heuristic_label();
            (label != "on_track").then(|| worker.next_step(label))
        })
        .collect();
    decision.next_steps = wake_steps(snapshot, worker_steps);
    decision.wake = !snapshot.healthy_and_unchanged() && !decision.next_steps.is_empty();
    if !decision.wake {
        decision.tick_label = "nothing_changed".into();
        decision.reason = if snapshot.healthy_and_unchanged() {
            "healthy_snapshot"
        } else {
            "no_next_step"
        };
    }
    decision
}

fn wake_steps(snapshot: &Snapshot, mut worker_steps: Vec<String>) -> Vec<String> {
    let mut steps: Vec<String> = snapshot
        .hard
        .iter()
        .map(|hard| hard.next_step(&snapshot.plan_id))
        .collect();
    steps.append(&mut worker_steps);
    steps.extend(snapshot.actionable_tasks.iter().map(|id| {
        format!(
            "inspect actionable task {id} with codeterm plan state {} and choose its next action",
            snapshot.plan_id
        )
    }));
    steps.extend(
        snapshot
            .delta
            .keys()
            .filter_map(|key| {
                key.strip_prefix("task:")
                    .map(|id| HardWake::TaskState(id.into()))
                    .or_else(|| {
                        key.strip_prefix("report:")
                            .map(|id| HardWake::NewReport(id.into()))
                    })
            })
            .map(|event| event.next_step(&snapshot.plan_id)),
    );
    steps
}

/// A decision that never wakes; heuristic worker labels are journal context only.
pub(super) fn suppressed(snapshot: &Snapshot, decided_by: &str, reason: &'static str) -> Decision {
    Decision {
        wake: false,
        tick_label: "nothing_changed".into(),
        decided_by: decided_by.into(),
        reason,
        observation_id: String::new(),
        snapshot_hash: snapshot.hash(),
        next_steps: Vec::new(),
        worker_labels: snapshot
            .workers
            .iter()
            .map(|worker| (worker.tab_id.clone(), worker.heuristic_label().to_string()))
            .collect(),
        learning_ids: None,
        scope: snapshot.scope.clone(),
        adapter: None,
        model: None,
    }
}
