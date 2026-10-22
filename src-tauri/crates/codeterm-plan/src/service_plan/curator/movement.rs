use super::digest::Row;
use crate::plan::{Plan, TaskStatus};
use std::collections::BTreeMap;

pub fn rows(plan: &Plan, since: u64, now: u64) -> BTreeMap<String, Vec<Row>> {
    let mut rows = BTreeMap::<String, Vec<Row>>::new();
    let mut review_count = 0;
    let mut oldest_review = 0;
    for task in plan.tasks.iter().filter(|task| !task.status.is_terminal()) {
        let age = now.saturating_sub(task.status_changed_at.max(task.created_at));
        rows.entry("time in state".into()).or_default().push(Row {
            id: task.id.clone(),
            kind: "task".into(),
            state: task.status.key().into(),
            summary: format!("{age:020}"),
        });
        if task.status == TaskStatus::InReview {
            review_count += 1;
            oldest_review = oldest_review.max(age);
        }
        let mut waits = task
            .depends_on
            .iter()
            .filter(|id| {
                plan.tasks
                    .iter()
                    .any(|dep| &dep.id == *id && !dep.status.is_terminal())
            })
            .cloned()
            .collect::<Vec<_>>();
        waits.extend(
            plan.gates
                .iter()
                .filter(|gate| gate.is_open() && gate.blocking_task_ids.contains(&task.id))
                .map(|gate| gate.id.clone()),
        );
        if task.status == TaskStatus::InReview {
            waits.push(
                plan.owner
                    .tab_id
                    .clone()
                    .unwrap_or_else(|| "orchestrator".into()),
            );
        }
        if task.status == TaskStatus::Blocked {
            waits.push("blocker".into());
        }
        if task.status == TaskStatus::Paused {
            waits.push("machine_hold".into());
        }
        if !waits.is_empty() {
            waits.sort();
            waits.dedup();
            rows.entry("waits".into()).or_default().push(Row {
                id: task.id.clone(),
                kind: "wait".into(),
                state: task.status.key().into(),
                summary: waits.join(" "),
            });
        }
    }
    rows.entry("review backlog".into()).or_default().push(Row {
        id: plan.id.clone(),
        kind: "review".into(),
        state: review_count.to_string(),
        summary: oldest_review.to_string(),
    });
    for gate in &plan.gates {
        if gate.is_open() {
            rows.entry("owner questions".into()).or_default().push(Row {
                id: gate.id.clone(),
                kind: "gate".into(),
                state: "open".into(),
                summary: now.saturating_sub(gate.asked_at).to_string(),
            });
        }
        if let Some(answer) = gate.answer.as_ref().filter(|answer| answer.at > since) {
            rows.entry("decisions".into()).or_default().push(Row {
                id: gate.id.clone(),
                kind: "answer".into(),
                state: "answered".into(),
                summary: answer.at.to_string(),
            });
        }
    }
    for stride in plan.strides.iter().filter(|stride| {
        stride.ts > since
            && matches!(
                stride.kind.as_str(),
                "decision" | "plan_decision" | "decision_answer"
            )
    }) {
        rows.entry("decisions".into()).or_default().push(Row {
            id: stride.id.clone(),
            kind: "decision".into(),
            state: "recorded".into(),
            summary: stride.ts.to_string(),
        });
    }
    rows
}
