use serde::Serialize;
use std::collections::HashSet;

use crate::plan::{AgentRef, Milestone, Plan, TaskStatus};

use super::mutations::plan_contains_id;
use super::{milestone_gates, PlanError, PlanStore};

/// Default cadence for the gate/bounce reflection trigger (`reflect.every_gates`).
pub const DEFAULT_REFLECT_EVERY_GATES: u32 = 15;

/// Kind for a bounce working point, when one is recorded. Bounces are not yet a
/// first-class event, so the window count stays an honest zero until they are.
pub const BOUNCE_MILESTONE_KIND: &str = "bounce";

/// Kind for an explicit `plan groups close` stride — the alternative to the
/// derived (every member task reaching `done`) group-completion signal. Lets an
/// Orchestrator close a group's reflection window before every task is done.
pub const GROUP_CLOSE_MILESTONE_KIND: &str = "group_close";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReflectionWindow {
    pub tasks_closed: u32,
    pub bounces: u32,
    pub gate_leaks: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReflectionTrigger {
    GroupClose { key: String, label: String },
    GateCadence { events: u32, every: u32 },
}

impl ReflectionTrigger {
    pub fn label(&self) -> String {
        match self {
            ReflectionTrigger::GroupClose { label, .. } => {
                format!("group {label} closed")
            }
            ReflectionTrigger::GateCadence { events, every } => {
                format!("{events} gate/bounce events (every {every})")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Reflection {
    pub trigger: ReflectionTrigger,
    pub window: ReflectionWindow,
}

// A recorded heartbeat learning is the silencer: it routes the lesson to an
// existing primitive AND advances the reflection clock past this window.
fn latest_learning_ts(plan: &Plan) -> u64 {
    plan.heartbeat
        .learnings
        .iter()
        .map(|l| l.ts)
        .max()
        .unwrap_or(0)
}

// Gate/bounce event timestamps, ascending. Decision gates are first-class;
// bounces are counted where recorded (honest undercount, never fabricated).
fn gate_bounce_event_ts(plan: &Plan) -> Vec<u64> {
    let mut ts: Vec<u64> = plan.gates.iter().map(|g| g.asked_at).collect();
    for task in &plan.tasks {
        for m in &task.milestones {
            if m.kind == BOUNCE_MILESTONE_KIND {
                ts.push(m.ts);
            }
        }
    }
    for m in &plan.strides {
        if m.kind == BOUNCE_MILESTONE_KIND {
            ts.push(m.ts);
        }
    }
    ts.sort_unstable();
    ts
}

pub fn gate_bounce_event_count(plan: &Plan) -> u32 {
    gate_bounce_event_ts(plan).len() as u32
}

// Timestamp of the most recent N-multiple event, or None before the first window.
fn latest_cadence_boundary_ts(plan: &Plan, every: u32) -> Option<u64> {
    if every == 0 {
        return None;
    }
    let ts = gate_bounce_event_ts(plan);
    let count = ts.len() as u32;
    if count < every {
        return None;
    }
    let boundary = (count / every) * every;
    ts.get(boundary as usize - 1).copied()
}

// An explicit `plan groups close` stride, keyed by the same `group:<key>` link
// convention `milestone_gates::regression_link` already uses.
fn explicit_group_close_ts(plan: &Plan, key: &str) -> Option<u64> {
    let link = milestone_gates::regression_link(key);
    plan.strides
        .iter()
        .filter(|m| m.kind == GROUP_CLOSE_MILESTONE_KIND && m.links.iter().any(|l| l == &link))
        .map(|m| m.ts)
        .max()
}

// A group closes for reflection purposes either derived (every member task
// terminal, at least one done) or explicit (`plan groups close`) — whichever
// is later, so re-deriving after an explicit close still re-arms correctly.
fn group_close_ts(plan: &Plan, key: &str) -> Option<u64> {
    let derived = milestone_gates::group_completion(plan, key);
    let explicit = explicit_group_close_ts(plan, key);
    match (derived, explicit) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(a.max(b)),
    }
}

fn reflection_window(plan: &Plan, since: u64) -> ReflectionWindow {
    let tasks_closed = plan
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done && t.status_changed_at > since)
        .count() as u32;
    let bounces = plan
        .tasks
        .iter()
        .flat_map(|t| t.milestones.iter())
        .chain(plan.strides.iter())
        .filter(|m| m.kind == BOUNCE_MILESTONE_KIND && m.ts > since)
        .count() as u32;
    let gate_leaks = milestone_gates::pending_regression_gates(plan).len() as u32;
    ReflectionWindow {
        tasks_closed,
        bounces,
        gate_leaks,
    }
}

/// Pending reflections since the last recorded learning: one per group closed
/// after that point, plus one when a new gate/bounce cadence window is crossed.
/// One-shot — recording a learning advances `since` and silences everything up
/// to it; a later group close or cadence boundary re-arms.
pub fn pending_reflections(plan: &Plan, every_gates: u32) -> Vec<Reflection> {
    let since = latest_learning_ts(plan);
    let window = reflection_window(plan, since);
    let mut out = Vec::new();

    let mut seen = HashSet::new();
    let mut keys: Vec<String> = Vec::new();
    for task in &plan.tasks {
        if let Some(key) = task.group.as_deref() {
            if seen.insert(key.to_string()) {
                keys.push(key.to_string());
            }
        }
    }
    // Declared groups with no member yet still admit an explicit close.
    for group in &plan.task_groups {
        if seen.insert(group.key.clone()) {
            keys.push(group.key.clone());
        }
    }
    for key in &keys {
        let Some(completed_at) = group_close_ts(plan, key) else {
            continue;
        };
        if completed_at <= since {
            continue;
        }
        out.push(Reflection {
            trigger: ReflectionTrigger::GroupClose {
                key: key.clone(),
                label: group_label(plan, key),
            },
            window: window.clone(),
        });
    }
    out.sort_by(|a, b| trigger_sort_key(&a.trigger).cmp(&trigger_sort_key(&b.trigger)));

    if let Some(boundary) = latest_cadence_boundary_ts(plan, every_gates) {
        if boundary > since {
            out.push(Reflection {
                trigger: ReflectionTrigger::GateCadence {
                    events: gate_bounce_event_count(plan),
                    every: every_gates,
                },
                window,
            });
        }
    }
    out
}

fn trigger_sort_key(trigger: &ReflectionTrigger) -> String {
    match trigger {
        ReflectionTrigger::GroupClose { key, .. } => key.clone(),
        ReflectionTrigger::GateCadence { .. } => String::new(),
    }
}

fn group_label(plan: &Plan, key: &str) -> String {
    plan.task_groups
        .iter()
        .find(|g| g.key == key)
        .map(|g| g.label.clone())
        .unwrap_or_else(|| key.to_string())
}

/// Render the one-shot reflection block into the orchestrator heartbeat.
pub fn render_reflection(plan: &Plan, every_gates: u32, out: &mut String) {
    let pending = pending_reflections(plan, every_gates);
    let Some(first) = pending.first() else {
        return;
    };
    out.push_str(super::prompts_plan::REFLECTION_HEARTBEAT_TITLE);
    out.push('\n');
    out.push_str("- ");
    out.push_str(&super::prompts_plan::reflection_block(
        &first.trigger.label(),
        &first.window,
    ));
    out.push('\n');
}

impl PlanStore {
    /// Explicit alternative to the derived (last-member-task-done) group
    /// closure: lets an Orchestrator close a group's reflection window before
    /// every task in it reaches `done` (e.g. residual polish deliberately
    /// deferred). Records a stride — the existing milestone primitive, no new
    /// store — linked to the group the same way a regression run is.
    pub fn close_group(
        &self,
        plan_id: &str,
        group_key: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        self.with_plan(plan_id, now_ms, |plan| {
            let group = plan
                .task_groups
                .iter()
                .find(|g| g.key == group_key)
                .ok_or(PlanError::GroupNotFound)?;
            let link = milestone_gates::regression_link(&group.key);
            let stride_id =
                crate::plan::ids::mint_id("ms", |candidate| plan_contains_id(plan, candidate));
            plan.strides.push(Milestone {
                id: stride_id,
                from,
                kind: GROUP_CLOSE_MILESTONE_KIND.into(),
                text: note.into(),
                ts: now_ms,
                links: vec![link],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
