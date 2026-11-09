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
                acceptance_criteria: vec!["opaque-acceptance-61".into()],
                test_plan: Some("opaque-test-plan-62".into()),
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

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

    assert!(out.contains("<codeterm-worker-charter"));
    assert!(out.contains(&task_id));
    assert!(out.contains(prompts_plan::TESTING_DISCIPLINE.trim()));
    assert!(out.contains(prompts_plan::QA_PIPELINE_DISCIPLINE.trim()));
    assert!(out.trim_end().ends_with("continue"));
}

#[test]
fn charter_uses_generic_pointer_when_plan_has_no_skills() {
    let (_dir, store, _task_id) = store_with_assigned_task();

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

    assert!(out.contains("<codeterm-worker-charter"));
    assert!(out.trim_end().ends_with("continue"));
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
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ntitle: Seed fixtures\n---\n\nopaque-skill-body-63\n",
    )
    .unwrap();
    std::fs::write(
        skill_dir.parent().unwrap().join("ledger.jsonl"),
        r#"{"seq":1,"slug":"seed-fixtures","verdict":"keep","rationale":"fixture"}
"#,
    )
    .unwrap();

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

    assert!(out.contains(prompts_plan::PLAN_SKILLS_WORKER_LINE));
    assert!(!out.contains(super::super::skill_registry::PLAN_SKILL_REGISTRY_POINTER));
    assert!(!out.contains("opaque-skill-body-63"));
}

#[test]
fn prepends_loop_context_for_loop_targeted_assignment() {
    let (_dir, store, task_id) = store_with_assigned_task();
    store
        .edit_plan_fixture("pl_charter", 3, |plan| {
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

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

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
    assert!(out.contains(&loop_id_line(&LoopCharterRead::from(&loop_spec))));
    assert!(out.contains(&loop_iteration_line(&LoopCharterRead::from(&loop_spec))));
    assert!(out.contains(&loop_stop_when_line(&LoopCharterRead::from(&loop_spec))));
}

#[test]
fn no_stride_block_when_task_has_no_relevant_strides() {
    let (_dir, store, _task_id) = store_with_assigned_task();

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

    assert!(!out.contains("use approach A"));
}

#[test]
fn prepends_pinned_decision_stride_for_assigned_tab() {
    let (_dir, store, task_id) = store_with_assigned_task();
    store
        .edit_plan_fixture("pl_charter", 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .expect("task present");
            task.milestones.push(crate::plan::Milestone {
                id: "ms_dec".into(),
                from: crate::plan::AgentRef::from_tab("orchestrator"),
                kind: "decision".into(),
                text: "opaque-decision-64".into(),
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

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

    assert_eq!(out.matches("opaque-decision-64").count(), 1);
}

#[test]
fn prepends_competitive_context_without_leaking_other_attempt_ids() {
    let (_dir, store, task_id) = store_with_assigned_task();
    // Sequential assignment replaces rather than competes: it closes the
    // outgoing attempt, leaving nothing for the charter to warn about.
    let drafts = store
        .build_task_compete_drafts(
            "pl_charter",
            &task_id,
            crate::service_plan::task_attempts::TaskCompeteRequest::default(),
        )
        .unwrap();
    store
        .record_task_compete_attempt(
            "pl_charter",
            &task_id,
            "worker-tab",
            &drafts[0],
            Some("worker"),
            3,
        )
        .unwrap();
    store
        .record_task_compete_attempt(
            "pl_charter",
            &task_id,
            "worker-tab-2",
            &drafts[1],
            Some("worker"),
            4,
        )
        .unwrap();

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

    assert!(!out.contains("worker-tab-2"));
}

#[test]
fn noop_for_unassigned_tab() {
    let (_dir, store, _task_id) = store_with_assigned_task();

    let out = store
        .prepend_worker_charter_for_tab("some-other-tab", "continue")
        .unwrap();

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

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

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

    let out = store
        .prepend_worker_charter_for_tab("worker-tab", "continue")
        .unwrap();

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

    let out = store
        .prepend_worker_charter_for_tab("dual-role-tab", "continue")
        .unwrap();

    assert_eq!(out, "continue");
}
