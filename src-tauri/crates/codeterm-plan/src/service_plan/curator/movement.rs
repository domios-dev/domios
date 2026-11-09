use super::digest::Row;
use crate::plan::{Plan, TaskStatus};
use std::collections::BTreeMap;
mod inputs;
pub use inputs::{
    append_gate_rows, append_task_rows, review_backlog_row, task_age, TaskMovementInput,
};

pub fn current_rows(plan: &Plan, since: u64, now: u64) -> BTreeMap<String, Vec<Row>> {
    let mut rows = BTreeMap::<String, Vec<Row>>::new();
    let mut review_count = 0;
    let mut oldest_review = 0;
    for task in plan.tasks.iter().filter(|task| !task.status.is_terminal()) {
        let age = task_age(task.created_at, task.status_changed_at, now);
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
        append_task_rows(
            &mut rows,
            TaskMovementInput {
                id: &task.id,
                status: &task.status,
                created_at: task.created_at,
                status_changed_at: task.status_changed_at,
                waits,
                owner: plan.owner.tab_id.as_deref(),
            },
            now,
        );
    }
    rows.entry("review backlog".into())
        .or_default()
        .push(review_backlog_row(&plan.id, review_count, oldest_review));
    for gate in &plan.gates {
        append_gate_rows(
            &mut rows,
            &gate.id,
            &gate.status,
            gate.asked_at,
            gate.answer.as_ref().map(|answer| answer.at),
            since,
            now,
        );
    }
    rows
}

pub fn rows(plan: &Plan, since: u64, now: u64) -> BTreeMap<String, Vec<Row>> {
    let mut rows = current_rows(plan, since, now);
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
