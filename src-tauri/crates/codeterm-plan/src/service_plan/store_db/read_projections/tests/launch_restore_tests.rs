use super::*;
use crate::plan::{PlanMode, PlanStatus, PlannerRef, TaskStatus};

#[test]
fn launch_restore_readers_select_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.canonical_project_path = "/canonical".into();
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].lane.worktree_path = Some("/lane".into());
        plan.tasks[0].lane.base = Some("lane-base".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: None,
            title: None,
        });
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let task = database
            .tab_launch_task_projection(&plan.id, &plan.tasks[0].id, "worker")
            .unwrap()
            .unwrap();
        let lane = database
            .launch_lane_projection(&plan.id, &plan.tasks[0].id)
            .unwrap()
            .unwrap();
        let continuing = database
            .continuing_restore_tab_projection("worker")
            .unwrap();
        let owner = database.plan_identity_projection(&plan.id).unwrap();
        let binding = database
            .assigned_task_binding_projection("worker", false)
            .unwrap()
            .unwrap();
        let status = database
            .task_status_projection(&binding.plan_id, &binding.task_id)
            .unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(task.attempt_id.as_deref(), Some("current"));
        assert_eq!(task.base.as_deref(), Some("lane-base"));
        assert_eq!(lane.project_root, "/canonical");
        assert!(continuing);
        assert_eq!(owner.owner, plan.owner);
        assert_eq!(status, TaskStatus::InProgress);
        assert_eq!(metrics.queries, 6);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(task, lane, continuing, owner, binding, status))
            .unwrap()
            .len();
        println!("fixture={} consumers=tab_task_env,canonical_project_for_task_lane,lazy_restore_initialize,supporter_worker_owner,supporter_delivery_target,gateway_post_submit_assignment tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn launch_task_env_preserves_reverse_open_attempt_and_blank_base_precedence() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.tasks[0].lane.base = Some("lane".into());
    let original = plan.tasks[0].execution_attempts[0].clone();
    let mut last = original.clone();
    last.id = "last".into();
    last.base = Some(String::new());
    plan.tasks[0].execution_attempts.push(last.clone());
    let mut retired = last.clone();
    retired.id = "retired".into();
    retired.retired_at = Some(1);
    plan.tasks[0].execution_attempts.push(retired);
    let mut closed = last;
    closed.id = "closed".into();
    closed.closed_at = Some(1);
    plan.tasks[0].execution_attempts.push(closed);
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .tab_launch_task_projection(&plan.id, &plan.tasks[0].id, "worker")
        .unwrap()
        .unwrap();
    assert_eq!(read.attempt_id.as_deref(), Some("last"));
    assert_eq!(read.base.as_deref(), Some(""));
    let missing = database
        .tab_launch_task_projection(&plan.id, &plan.tasks[0].id, "other")
        .unwrap()
        .unwrap();
    assert!(missing.attempt_id.is_none());
    assert_eq!(missing.base.as_deref(), Some("lane"));
    assert!(database
        .tab_launch_task_projection("missing", "task", "worker")
        .unwrap()
        .is_none());
    plan.canonical_project_path = "\u{2003}".into();
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .launch_lane_projection(&plan.id, &plan.tasks[0].id)
            .unwrap()
            .unwrap()
            .project_root,
        plan.project_path
    );
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database
        .tab_launch_task_projection(&plan.id, &plan.tasks[0].id, "worker")
        .is_err());
    assert!(database
        .continuing_restore_tab_projection("worker")
        .is_err());
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plans")
        .unwrap();
    assert!(database
        .launch_lane_projection(&plan.id, &plan.tasks[0].id)
        .is_err());
}

#[test]
fn continuing_restore_keeps_owner_planner_remote_and_retirement_rules_per_tab() {
    for variant in 0..8 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), 0, 1);
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: None,
            title: None,
        });
        match variant {
            1 => plan.mode = PlanMode::Paused,
            2 => plan.mode = PlanMode::Complete,
            3 => plan.status = PlanStatus::Archived,
            4 => plan.tasks[0].execution_attempts[0].mesh_peer = Some("remote".into()),
            5 => plan.tasks[0].execution_attempts[0].retired_at = Some(1),
            6 => plan.tasks[0].execution_attempts[0].closed_at = Some(1),
            7 => plan.tasks[0].status = TaskStatus::Paused,
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let continuing = plan.status == PlanStatus::Active
            && !matches!(plan.mode, PlanMode::Paused | PlanMode::Complete)
            && plan.tasks[0].status == TaskStatus::InProgress
            && plan.tasks[0].execution_attempts[0].closed_at.is_none()
            && plan.tasks[0].execution_attempts[0].retired_at.is_none();
        assert_eq!(
            database
                .continuing_restore_tab_projection("worker")
                .unwrap(),
            continuing && variant != 4,
            "variant={variant}"
        );
        assert_eq!(
            database.continuing_restore_tab_projection("owner").unwrap(),
            continuing,
            "variant={variant}"
        );
        assert_eq!(
            database
                .continuing_restore_tab_projection("planner")
                .unwrap(),
            continuing,
            "variant={variant}"
        );
        assert!(!database
            .continuing_restore_tab_projection("unbound")
            .unwrap());
    }
}
