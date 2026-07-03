use super::PlanStore;
use crate::plan::{LoopSpec, LoopStatus, Plan, PlanMode, PlanStatus, TaskStatus};

#[derive(Debug, Clone)]
struct WorkerAssignmentContext {
    task_id: String,
    role_guidance: Option<String>,
    loop_spec: Option<LoopSpec>,
    live_attempt_count: usize,
}

fn loop_for_task(plan: &Plan, task_id: &str) -> Option<LoopSpec> {
    plan.loops
        .iter()
        .find(|loop_spec| {
            loop_spec.status == LoopStatus::Running && loop_spec.target.trim() == task_id
        })
        .cloned()
}

fn loop_stop_when_text(loop_spec: &LoopSpec) -> &str {
    loop_spec
        .stop_when
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(none recorded)")
}

pub(super) fn loop_id_line(loop_spec: &LoopSpec) -> String {
    format!("- loop_id: {}", loop_spec.id)
}

pub(super) fn loop_iteration_line(loop_spec: &LoopSpec) -> String {
    format!("- iteration: {}/{}", loop_spec.count, loop_spec.budget)
}

pub(super) fn loop_stop_when_line(loop_spec: &LoopSpec) -> String {
    format!("- stop_when: {}", loop_stop_when_text(loop_spec))
}

pub(super) fn competitive_attempt_count_line(live_attempt_count: usize) -> String {
    format!("- this task currently has {live_attempt_count} live attempts.")
}

fn render_loop_context(loop_spec: &LoopSpec) -> String {
    format!(
        "{}\n{}\n{}\n{}\n- {}\n",
        crate::prompts::WORKER_LOOP_CONTEXT_TITLE,
        loop_id_line(loop_spec),
        loop_iteration_line(loop_spec),
        loop_stop_when_line(loop_spec),
        crate::prompts::WORKER_LOOP_EVIDENCE_RULE
    )
}

fn render_competitive_context(live_attempt_count: usize) -> String {
    let mut out = format!(
        "{}\n{}\n",
        crate::prompts::WORKER_COMPETITIVE_CONTEXT_TITLE,
        competitive_attempt_count_line(live_attempt_count)
    );
    for rule in crate::prompts::WORKER_COMPETITIVE_ISOLATION_RULES {
        out.push_str("- ");
        out.push_str(rule);
        out.push('\n');
    }
    out
}

/// Compact, self-sufficient restatement of the three Worker rules. Mirrors the
/// orchestrator-heartbeat reinjection so worker role survives context drift.
pub fn render_worker_charter(
    task_id: &str,
    loop_spec: Option<&LoopSpec>,
    live_attempt_count: usize,
) -> String {
    let mut out = format!(
        "<codeterm-worker-charter task=\"{task_id}\">\n\
You are a Worker on task {task_id}. Three rules:\n\
- Do this ONE task, then run `codeterm agent report` and exit.\n\
- NEVER spawn agents.\n\
- Decomposition needed \u{2192} report blocked; the Orchestrator splits it.\n\
",
    );
    if let Some(loop_spec) = loop_spec {
        out.push_str(&render_loop_context(loop_spec));
    }
    if live_attempt_count > 1 {
        out.push_str(&render_competitive_context(live_attempt_count));
    }
    out.push_str(crate::prompts::TESTING_DISCIPLINE.trim());
    out.push('\n');
    out.push_str(crate::prompts::QA_PIPELINE_DISCIPLINE.trim());
    out.push_str("\n</codeterm-worker-charter>");
    out
}

fn report_is_pending(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
    )
}

impl PlanStore {
    /// Resolve the open task assigned to this pane across active plans. Newest
    /// matching active plan wins; unknown panes resolve to None.
    fn open_task_for_pane(&self, pane_id: &str) -> Option<WorkerAssignmentContext> {
        if pane_id.trim().is_empty() {
            return None;
        }
        let mut plans = self.list_full();
        plans.sort_by_key(|plan| std::cmp::Reverse(plan.updated_at));
        for plan in plans {
            if plan.status != PlanStatus::Active || plan.mode != PlanMode::Execution {
                continue;
            }
            if let Some(task) = plan.tasks.iter().find(|task| {
                report_is_pending(&task.status)
                    && (task.assigned_pane_id.as_deref() == Some(pane_id)
                        || task.lane.pane_id.as_deref() == Some(pane_id)
                        || task.execution_attempts.iter().any(|attempt| {
                            attempt.closed_at.is_none() && attempt.executor_pane_id == pane_id
                        }))
            }) {
                return Some(WorkerAssignmentContext {
                    task_id: task.id.clone(),
                    role_guidance: task.role_guidance.clone(),
                    loop_spec: loop_for_task(&plan, &task.id),
                    live_attempt_count: super::task_attempts::live_attempt_count(task),
                });
            }
        }
        None
    }

    /// Prepend the worker charter for a pane currently assigned to an open task.
    /// No-op (returns `text` unchanged) for panes with no such assignment —
    /// the orchestrator-heartbeat prepend is the symmetric counterpart.
    pub fn prepend_worker_charter_for_pane(&self, pane_id: &str, text: &str) -> String {
        if self
            .active_owned_plan_with_open_work_for_pane(pane_id)
            .is_some()
        {
            return text.to_string();
        }
        let Some(context) = self.open_task_for_pane(pane_id) else {
            return text.to_string();
        };
        let mut charter = render_worker_charter(
            &context.task_id,
            context.loop_spec.as_ref(),
            context.live_attempt_count,
        );
        if let Some(guidance) = context
            .role_guidance
            .as_deref()
            .filter(|guidance| !guidance.trim().is_empty())
        {
            charter = format!("Role guidance:\n{}\n\n{charter}", guidance.trim());
        }
        if text.trim().is_empty() {
            charter
        } else {
            format!("{charter}\n\n{text}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{LoopSpec, LoopStatus, OrchestratorRef, Plan, PlanMode};
    use crate::service::plan::NewTask;

    fn store_with_assigned_task() -> (tempfile::TempDir, PlanStore, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Charter".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_charter".into();
        plan.mode = PlanMode::Execution;
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Do the thing".into(),
                    kind: "task".into(),
                    assigned_pane_id: Some("worker-pane".into()),
                    competition: None,
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        (dir, store, task_id)
    }

    #[test]
    fn prepends_charter_for_assigned_pane() {
        let (_dir, store, task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_pane("worker-pane", "continue");

        assert!(out.contains("<codeterm-worker-charter"));
        assert!(out.contains(&task_id));
        assert!(out.contains("NEVER spawn agents"));
        assert!(out.contains(crate::prompts::TESTING_DISCIPLINE.trim()));
        assert!(out.contains(crate::prompts::QA_PIPELINE_DISCIPLINE.trim()));
        assert!(out.trim_end().ends_with("continue"));
    }

    #[test]
    fn prepends_loop_context_for_loop_targeted_assignment() {
        let (_dir, store, task_id) = store_with_assigned_task();
        store
            .with_plan("pl_charter", 3, |plan| {
                plan.loops.push(LoopSpec {
                    id: "loop_focus".into(),
                    target: task_id.clone(),
                    budget: 5,
                    count: 2,
                    stop_when: Some("qa passes".into()),
                    status: LoopStatus::Running,
                    stagnation: 0,
                    deviation: None,
                    last_iteration_at: None,
                });
                Ok(())
            })
            .unwrap();

        let out = store.prepend_worker_charter_for_pane("worker-pane", "continue");

        let loop_spec = LoopSpec {
            id: "loop_focus".into(),
            target: task_id,
            budget: 5,
            count: 2,
            stop_when: Some("qa passes".into()),
            status: LoopStatus::Running,
            stagnation: 0,
            deviation: None,
            last_iteration_at: None,
        };
        assert!(out.contains(crate::prompts::WORKER_LOOP_CONTEXT_TITLE));
        assert!(out.contains(&loop_id_line(&loop_spec)));
        assert!(out.contains(&loop_iteration_line(&loop_spec)));
        assert!(out.contains(&loop_stop_when_line(&loop_spec)));
        assert!(out.contains(crate::prompts::WORKER_LOOP_EVIDENCE_RULE));
    }

    #[test]
    fn prepends_competitive_context_without_leaking_other_attempt_ids() {
        let (_dir, store, task_id) = store_with_assigned_task();
        store
            .assign_task("pl_charter", &task_id, "worker-pane", None, 3)
            .unwrap();
        store
            .assign_task("pl_charter", &task_id, "worker-pane-2", None, 4)
            .unwrap();

        let out = store.prepend_worker_charter_for_pane("worker-pane", "continue");

        assert!(out.contains(crate::prompts::WORKER_COMPETITIVE_CONTEXT_TITLE));
        assert!(out.contains(&competitive_attempt_count_line(2)));
        for rule in crate::prompts::WORKER_COMPETITIVE_ISOLATION_RULES {
            assert!(out.contains(rule));
        }
        assert!(!out.contains("worker-pane-2"));
    }

    #[test]
    fn noop_for_unassigned_pane() {
        let (_dir, store, _task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_pane("some-other-pane", "continue");

        assert_eq!(out, "continue");
    }

    #[test]
    fn noop_for_planning_plan_assignment() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Draft charter".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_draft_charter".into();
        store.save(&mut plan, 1).unwrap();
        store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Draft task".into(),
                    kind: "task".into(),
                    assigned_pane_id: Some("worker-pane".into()),
                    competition: None,
                    ..Default::default()
                },
                2,
            )
            .unwrap();

        let out = store.prepend_worker_charter_for_pane("worker-pane", "continue");

        assert_eq!(out, "continue");
    }

    #[test]
    fn noop_after_worker_reported_in_review() {
        let (_dir, store, task_id) = store_with_assigned_task();
        store
            .update_task(
                "pl_charter",
                &task_id,
                None,
                None,
                None,
                None,
                Some("in_review"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                3,
            )
            .unwrap();

        let out = store.prepend_worker_charter_for_pane("worker-pane", "continue");

        assert_eq!(out, "continue");
    }

    #[test]
    fn noop_for_pane_that_owns_active_plan_with_open_work() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut owner_plan = Plan::new(
            "Owned plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("dual-role-pane"),
        );
        owner_plan.id = "pl_owned_charter".into();
        owner_plan.mode = PlanMode::Execution;
        store.save(&mut owner_plan, 1).unwrap();
        store
            .add_task(
                &owner_plan.id,
                NewTask {
                    name: "Coordinate".into(),
                    kind: "task".into(),
                    competition: None,
                    ..Default::default()
                },
                2,
            )
            .unwrap();

        let mut worker_plan = Plan::new(
            "Assigned plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("other-orch"),
        );
        worker_plan.id = "pl_assigned_charter".into();
        worker_plan.mode = PlanMode::Execution;
        store.save(&mut worker_plan, 3).unwrap();
        store
            .add_task(
                &worker_plan.id,
                NewTask {
                    name: "Assigned task".into(),
                    kind: "task".into(),
                    assigned_pane_id: Some("dual-role-pane".into()),
                    competition: None,
                    ..Default::default()
                },
                4,
            )
            .unwrap();

        let out = store.prepend_worker_charter_for_pane("dual-role-pane", "continue");

        assert_eq!(out, "continue");
    }
}
