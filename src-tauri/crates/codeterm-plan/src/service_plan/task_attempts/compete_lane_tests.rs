use super::TaskCompeteDraft;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::{NewTask, PlanStore};

fn store_with_task() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "competition".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "compete".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, plan.id, task_id)
}

fn draft(plan_id: &str, task_id: &str, lane: u32, branch: Option<&str>) -> TaskCompeteDraft {
    TaskCompeteDraft {
        plan_id: plan_id.to_string(),
        task_id: task_id.to_string(),
        competition_group_id: "compete_x".into(),
        lane,
        provider: "claude".into(),
        planned_model: None,
        model: None,
        parent_tab_id: Some("orch".into()),
        workspace_id: None,
        title: None,
        cwd: format!("/lanes/c{lane}"),
        branch: branch.map(str::to_string),
        base: branch.map(|_| "abc123".to_string()),
        destination: Some("develop".into()),
        task_prompt: "brief".into(),
        report_command: "codeterm agent report".into(),
        plan_skill_manifest: vec![],
    }
}

#[test]
fn competitive_attempt_records_its_lane_provenance() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    let created = store
        .record_task_compete_attempt(
            &plan_id,
            &task_id,
            "tab-c1",
            &draft(&plan_id, &task_id, 1, Some("lane/compete-c1")),
            Some("worker"),
            3,
        )
        .unwrap();

    assert_eq!(created.attempt.worktree_path.as_deref(), Some("/lanes/c1"));
    assert_eq!(created.attempt.branch.as_deref(), Some("lane/compete-c1"));
    assert_eq!(created.attempt.base.as_deref(), Some("abc123"));
    assert_eq!(created.attempt.destination.as_deref(), Some("develop"));
    assert_eq!(created.attempt.competition_lane, Some(1));
}

/// Negative control: an unprovisioned draft records no lane fields, so read-only
/// competition keeps the exact attempt shape it has today.
#[test]
fn unprovisioned_competitive_attempt_records_no_lane() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    let created = store
        .record_task_compete_attempt(
            &plan_id,
            &task_id,
            "tab-c1",
            &draft(&plan_id, &task_id, 1, None),
            Some("worker"),
            3,
        )
        .unwrap();

    assert!(created.attempt.worktree_path.is_none());
    assert!(created.attempt.branch.is_none());
    assert!(created.attempt.base.is_none());
    assert!(created.attempt.destination.is_none());
}

#[test]
fn compete_drafts_carry_the_task_destination_and_no_lane_until_provisioned() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    let drafts = store
        .build_task_compete_drafts(
            &plan_id,
            &task_id,
            super::TaskCompeteRequest {
                attempts: Some(3),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(drafts.len(), 3);
    for draft in &drafts {
        assert!(draft.branch.is_none());
        assert!(draft.base.is_none());
    }
}
