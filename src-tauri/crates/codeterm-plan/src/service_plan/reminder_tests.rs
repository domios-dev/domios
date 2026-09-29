use super::{NewTask, PlanStore};
use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, PlanMode};

fn store_with_task(owner: &str) -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Reminder".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab(owner),
            vec![MilestoneKind {
                key: "status_update".into(),
                label: "Status update".into(),
                color: None,
            }],
            10,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "lane".into(),
                kind: "status_update".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("run focused test".into()),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 21).unwrap();
    (dir, store, plan.id, task_id)
}

#[test]
fn owner_execution_plan_with_open_work_is_nudge_eligible_without_task_binding() {
    let (_dir, store, plan_id, task_id) = store_with_task("orch-tab");
    store
        .update_task(
            &plan_id,
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
            30,
        )
        .unwrap();

    assert_eq!(store.task_binding_for_tab("orch-tab"), None);
    assert_eq!(
        store.active_owned_plan_with_open_work_for_tab("orch-tab"),
        Some(plan_id)
    );
}

#[test]
fn owner_planning_plan_with_open_work_is_not_nudge_eligible() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Planning".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-tab"),
            vec![MilestoneKind {
                key: "status_update".into(),
                label: "Status update".into(),
                color: None,
            }],
            10,
        )
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "draft lane".into(),
                kind: "status_update".into(),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();

    assert_eq!(
        store.active_owned_plan_with_open_work_for_tab("orch-tab"),
        None
    );
}

#[test]
fn paused_execution_plan_with_open_work_is_not_nudge_eligible() {
    let (_dir, store, plan_id, _task_id) = store_with_task("orch-tab");
    store
        .pause_plan(
            &plan_id,
            "provider quota exhausted",
            AgentRef::from_tab("orch-tab"),
            30,
        )
        .unwrap();

    assert_eq!(
        store.active_owned_plan_with_open_work_for_tab("orch-tab"),
        None
    );
}

#[test]
fn worker_planning_plan_binding_does_not_start_report_nudges() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Planning".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-tab"),
            vec![MilestoneKind {
                key: "status_update".into(),
                label: "Status update".into(),
                color: None,
            }],
            10,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "draft lane".into(),
                kind: "status_update".into(),
                assigned_tab_id: Some("worker-tab".into()),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();

    assert_eq!(
        store.task_binding_for_tab("worker-tab"),
        Some((plan.id.clone(), task_id))
    );
    assert_eq!(
        store.report_pending_task_binding_for_tab("worker-tab"),
        None
    );
}

#[test]
fn worker_pending_task_binding_is_nudge_eligible() {
    let (_dir, store, plan_id, task_id) = store_with_task("orch-tab");
    store
        .assign_task(&plan_id, &task_id, "worker-tab", None, 30)
        .unwrap();

    assert_eq!(
        store.task_binding_for_tab("worker-tab"),
        Some((plan_id.clone(), task_id.clone()))
    );
    assert_eq!(
        store.report_pending_task_binding_for_tab("worker-tab"),
        Some((plan_id, task_id))
    );
    assert_eq!(
        store.active_owned_plan_with_open_work_for_tab("worker-tab"),
        None
    );
}

#[test]
fn worker_in_review_task_binding_is_not_nudge_eligible_after_report() {
    let (_dir, store, plan_id, task_id) = store_with_task("orch-tab");
    store
        .assign_task(&plan_id, &task_id, "worker-tab", None, 30)
        .unwrap();
    store
        .update_task(
            &plan_id,
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
            40,
        )
        .unwrap();

    assert_eq!(
        store.active_owned_plan_with_open_work_for_tab("orch-tab"),
        Some(plan_id.clone())
    );
    assert_eq!(
        store.task_binding_for_tab("worker-tab"),
        Some((plan_id, task_id))
    );
    assert_eq!(
        store.report_pending_task_binding_for_tab("worker-tab"),
        None
    );
}

#[test]
fn terminal_done_and_cancelled_work_is_not_nudge_eligible() {
    let (_done_dir, done_store, done_plan_id, done_task_id) = store_with_task("done-orch");
    done_store.start_task_for_test(&done_plan_id, &done_task_id, 29);
    done_store
        .close_task(
            &done_plan_id,
            &done_task_id,
            None,
            AgentRef::from_tab("done-orch"),
            30,
        )
        .unwrap();

    let (_cancelled_dir, cancelled_store, cancelled_plan_id, cancelled_task_id) =
        store_with_task("cancelled-orch");
    cancelled_store
        .assign_task(
            &cancelled_plan_id,
            &cancelled_task_id,
            "worker-tab",
            None,
            25,
        )
        .unwrap();
    cancelled_store
        .update_task(
            &cancelled_plan_id,
            &cancelled_task_id,
            None,
            None,
            None,
            None,
            Some("cancelled"),
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
            30,
        )
        .unwrap();

    assert_eq!(
        done_store.active_owned_plan_with_open_work_for_tab("done-orch"),
        None
    );
    assert_eq!(
        cancelled_store.active_owned_plan_with_open_work_for_tab("cancelled-orch"),
        None
    );
    assert_eq!(
        cancelled_store.report_pending_task_binding_for_tab("worker-tab"),
        None
    );
}
