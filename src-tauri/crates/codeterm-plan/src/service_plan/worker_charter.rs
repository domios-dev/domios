use super::prompts_plan;
use super::PlanStore;
use crate::plan::{LoopSpec, LoopStatus, Plan, PlanMode, PlanStatus, TaskStatus};

#[derive(Debug, Clone)]
struct WorkerAssignmentContext {
    task_id: String,
    task_name: String,
    personality: Option<String>,
    role_guidance: Option<String>,
    strides: Option<String>,
    loop_spec: Option<LoopSpec>,
    live_attempt_count: usize,
    skill_count: usize,
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
        prompts_plan::WORKER_LOOP_CONTEXT_TITLE,
        loop_id_line(loop_spec),
        loop_iteration_line(loop_spec),
        loop_stop_when_line(loop_spec),
        prompts_plan::WORKER_LOOP_EVIDENCE_RULE
    )
}

fn render_competitive_context(live_attempt_count: usize) -> String {
    let mut out = format!(
        "{}\n{}\n",
        prompts_plan::WORKER_COMPETITIVE_CONTEXT_TITLE,
        competitive_attempt_count_line(live_attempt_count)
    );
    for rule in prompts_plan::WORKER_COMPETITIVE_ISOLATION_RULES {
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
    identity: Option<&str>,
    loop_spec: Option<&LoopSpec>,
    live_attempt_count: usize,
    skill_count: usize,
) -> String {
    let mut out = format!("<codeterm-worker-charter task=\"{task_id}\">\n");
    if let Some(identity) = identity {
        out.push_str(identity);
        out.push('\n');
        out.push('\n');
    }
    out.push_str(&format!(
        "You are a Worker on task {task_id}. Three rules:\n\
- Do this ONE task, then run `codeterm agent report` and exit.\n\
- NEVER spawn agents.\n\
- Decomposition needed \u{2192} report blocked; the Orchestrator splits it.\n\
"
    ));
    if skill_count >= 1 {
        out.push_str(prompts_plan::PLAN_SKILLS_WORKER_LINE);
    } else {
        out.push_str(super::skill_registry::PLAN_SKILL_REGISTRY_POINTER);
    }
    out.push('\n');
    if let Some(loop_spec) = loop_spec {
        out.push_str(&render_loop_context(loop_spec));
    }
    if live_attempt_count > 1 {
        out.push_str(&render_competitive_context(live_attempt_count));
    }
    out.push_str(prompts_plan::TESTING_DISCIPLINE.trim());
    out.push('\n');
    out.push_str(prompts_plan::QA_PIPELINE_DISCIPLINE.trim());
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
    /// Resolve the open task assigned to this tab across active plans. Newest
    /// matching active plan wins; unknown tabs resolve to None.
    fn open_task_for_tab(&self, tab_id: &str) -> Option<WorkerAssignmentContext> {
        if tab_id.trim().is_empty() {
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
                    && (task.assigned_tab_id.as_deref() == Some(tab_id)
                        || task.lane.tab_id.as_deref() == Some(tab_id)
                        || task.execution_attempts.iter().any(|attempt| {
                            attempt.closed_at.is_none() && attempt.executor_tab_id == tab_id
                        }))
            }) {
                return Some(WorkerAssignmentContext {
                    task_id: task.id.clone(),
                    task_name: task.name.clone(),
                    personality: task.personality.clone(),
                    role_guidance: task.role_guidance.clone(),
                    strides: super::stride_context::render_worker_strides(&task.milestones),
                    loop_spec: loop_for_task(&plan, &task.id),
                    live_attempt_count: super::task_attempts::live_attempt_count(task),
                    skill_count: super::skill_registry::count_plan_skills_for(&plan),
                });
            }
        }
        None
    }

    /// Prepend the worker charter for a tab currently assigned to an open task.
    /// No-op (returns `text` unchanged) for tabs with no such assignment —
    /// the orchestrator-heartbeat prepend is the symmetric counterpart.
    pub fn prepend_worker_charter_for_tab(&self, tab_id: &str, text: &str) -> String {
        if self
            .active_owned_plan_with_open_work_for_tab(tab_id)
            .is_some()
        {
            return text.to_string();
        }
        let Some(context) = self.open_task_for_tab(tab_id) else {
            return text.to_string();
        };
        let identity = super::worker_identity::compose_worker_identity(
            context.personality.as_deref(),
            context.role_guidance.as_deref(),
            Some(context.task_name.as_str()),
        );
        let mut charter = render_worker_charter(
            &context.task_id,
            identity.as_deref(),
            context.loop_spec.as_ref(),
            context.live_attempt_count,
            context.skill_count,
        );
        if let Some(strides) = context.strides.as_deref() {
            charter = format!("{charter}\n\n{strides}");
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
    use crate::service_plan::NewTask;

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
                    acceptance_criteria: vec!["it works".into()],
                    test_plan: Some("run focused test".into()),
                    assigned_tab_id: Some("worker-tab".into()),
                    competition: None,
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        (dir, store, task_id)
    }

    #[test]
    fn prepends_charter_for_assigned_tab() {
        let (_dir, store, task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert!(out.contains("<codeterm-worker-charter"));
        assert!(out.contains(&task_id));
        assert!(out.contains("NEVER spawn agents"));
        assert!(out.contains(prompts_plan::TESTING_DISCIPLINE.trim()));
        assert!(out.contains(prompts_plan::QA_PIPELINE_DISCIPLINE.trim()));
        assert!(out.trim_end().ends_with("continue"));
    }

    #[test]
    fn charter_uses_generic_pointer_when_plan_has_no_skills() {
        let (_dir, store, _task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert!(out.contains(super::super::skill_registry::PLAN_SKILL_REGISTRY_POINTER));
        assert!(!out.contains(prompts_plan::PLAN_SKILLS_WORKER_LINE));
    }

    #[test]
    fn charter_appends_worker_line_when_plan_has_skills() {
        let (_dir, store, _task_id) = store_with_assigned_task();
        let plan = store.get("pl_charter").unwrap();
        let skill_dir = crate::service_plan::paths::plan_state_dir_unchecked(
            crate::service_plan::paths::plan_sidecar_root(&plan),
            &plan.status,
            &plan.id,
        )
        .join("skills/seed-fixtures");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Seed fixtures").unwrap();
        std::fs::write(
            skill_dir.parent().unwrap().join("ledger.jsonl"),
            r#"{"seq":1,"slug":"seed-fixtures","verdict":"keep","rationale":"fixture"}
"#,
        )
        .unwrap();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert!(out.contains(prompts_plan::PLAN_SKILLS_WORKER_LINE));
        assert!(!out.contains(super::super::skill_registry::PLAN_SKILL_REGISTRY_POINTER));
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

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

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
        assert!(out.contains(prompts_plan::WORKER_LOOP_CONTEXT_TITLE));
        assert!(out.contains(&loop_id_line(&loop_spec)));
        assert!(out.contains(&loop_iteration_line(&loop_spec)));
        assert!(out.contains(&loop_stop_when_line(&loop_spec)));
        assert!(out.contains(prompts_plan::WORKER_LOOP_EVIDENCE_RULE));
    }

    #[test]
    fn no_stride_block_when_task_has_no_relevant_strides() {
        let (_dir, store, _task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert!(!out.contains("Recent task strides"));
    }

    #[test]
    fn prepends_pinned_decision_stride_for_assigned_tab() {
        let (_dir, store, task_id) = store_with_assigned_task();
        store
            .with_plan("pl_charter", 3, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == task_id)
                    .expect("task present");
                task.milestones.push(crate::plan::Milestone {
                    id: "ms_dec".into(),
                    from: crate::plan::AgentRef::from_tab("orchestrator"),
                    kind: "decision".into(),
                    text: "use approach A".into(),
                    ts: 10,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
                Ok(())
            })
            .unwrap();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert!(out.contains("Recent task strides"));
        assert!(out.contains("[decision] use approach A"));
    }

    #[test]
    fn prepends_competitive_context_without_leaking_other_attempt_ids() {
        let (_dir, store, task_id) = store_with_assigned_task();
        store
            .assign_task("pl_charter", &task_id, "worker-tab", None, 3)
            .unwrap();
        store
            .assign_task("pl_charter", &task_id, "worker-tab-2", None, 4)
            .unwrap();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert!(out.contains(prompts_plan::WORKER_COMPETITIVE_CONTEXT_TITLE));
        assert!(out.contains(&competitive_attempt_count_line(2)));
        for rule in prompts_plan::WORKER_COMPETITIVE_ISOLATION_RULES {
            assert!(out.contains(rule));
        }
        assert!(!out.contains("worker-tab-2"));
    }

    #[test]
    fn noop_for_unassigned_tab() {
        let (_dir, store, _task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_tab("some-other-tab", "continue");

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
                    assigned_tab_id: Some("worker-tab".into()),
                    competition: None,
                    ..Default::default()
                },
                2,
            )
            .unwrap();

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

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

        let out = store.prepend_worker_charter_for_tab("worker-tab", "continue");

        assert_eq!(out, "continue");
    }

    #[test]
    fn noop_for_tab_that_owns_active_plan_with_open_work() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut owner_plan = Plan::new(
            "Owned plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("dual-role-tab"),
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
            OrchestratorRef::from_tab("other-orch"),
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
                    assigned_tab_id: Some("dual-role-tab".into()),
                    competition: None,
                    ..Default::default()
                },
                4,
            )
            .unwrap();

        let out = store.prepend_worker_charter_for_tab("dual-role-tab", "continue");

        assert_eq!(out, "continue");
    }
}
