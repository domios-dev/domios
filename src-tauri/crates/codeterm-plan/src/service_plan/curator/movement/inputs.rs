use super::Row;
use crate::plan::{GateStatus, TaskStatus};
use std::collections::BTreeMap;

pub struct TaskMovementInput<'a> {
    pub id: &'a str,
    pub status: &'a TaskStatus,
    pub created_at: u64,
    pub status_changed_at: u64,
    pub waits: Vec<String>,
    pub owner: Option<&'a str>,
}

pub fn task_age(created_at: u64, status_changed_at: u64, now: u64) -> u64 {
    now.saturating_sub(status_changed_at.max(created_at))
}

pub fn append_task_rows(
    rows: &mut BTreeMap<String, Vec<Row>>,
    input: TaskMovementInput<'_>,
    now: u64,
) {
    if input.status.is_terminal() {
        return;
    }
    rows.entry("time in state".into()).or_default().push(Row {
        id: input.id.into(),
        kind: "task".into(),
        state: input.status.key().into(),
        summary: format!(
            "{:020}",
            task_age(input.created_at, input.status_changed_at, now)
        ),
    });
    let mut waits = input.waits;
    match input.status {
        TaskStatus::InReview => waits.push(input.owner.unwrap_or("orchestrator").into()),
        TaskStatus::Blocked => waits.push("blocker".into()),
        TaskStatus::Paused => waits.push("machine_hold".into()),
        _ => {}
    }
    if !waits.is_empty() {
        waits.sort();
        waits.dedup();
        rows.entry("waits".into()).or_default().push(Row {
            id: input.id.into(),
            kind: "wait".into(),
            state: input.status.key().into(),
            summary: waits.join(" "),
        });
    }
}

pub fn review_backlog_row(id: &str, count: usize, oldest: u64) -> Row {
    Row {
        id: id.into(),
        kind: "review".into(),
        state: count.to_string(),
        summary: oldest.to_string(),
    }
}

pub fn append_gate_rows(
    rows: &mut BTreeMap<String, Vec<Row>>,
    id: &str,
    status: &GateStatus,
    asked_at: u64,
    answer_at: Option<u64>,
    since: u64,
    now: u64,
) {
    if status.is_open() {
        rows.entry("owner questions".into()).or_default().push(Row {
            id: id.into(),
            kind: "gate".into(),
            state: "open".into(),
            summary: now.saturating_sub(asked_at).to_string(),
        });
    }
    if let Some(at) = answer_at.filter(|at| *at > since) {
        rows.entry("decisions".into()).or_default().push(Row {
            id: id.into(),
            kind: "answer".into(),
            state: "answered".into(),
            summary: at.to_string(),
        });
    }
}
