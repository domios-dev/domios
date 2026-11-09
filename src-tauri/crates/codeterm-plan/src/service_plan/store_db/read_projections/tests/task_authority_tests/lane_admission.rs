use super::*;
use crate::plan::TaskExecutionIntent;
use crate::service_plan::TaskLaneOwnerRead;

#[test]
fn lane_admission_selects_ordered_foreign_lane_facts_and_repository_intent() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.canonical_project_path = "\u{2003}".into();
    plan.tasks[0].lane.worktree_path = Some("own".into());
    for (id, handle, path) in [
        ("foreign-a", None, Some("first")),
        ("alias", Some("task_exact"), Some("excluded")),
        ("foreign-b", None, Some("")),
        ("no-lane", None, None),
    ] {
        let mut task = plan.tasks[0].clone();
        task.id = id.into();
        task.handle = handle.map(str::to_string);
        task.lane.worktree_path = path.map(str::to_string);
        plan.tasks.push(task);
    }
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .task_lane_admission_projection(&plan.id, "task_exact")
        .unwrap();
    assert_eq!(read.canonical_path, plan.project_path);
    assert_eq!(
        read.execution_intent,
        TaskExecutionIntent::RepositoryMutation
    );
    assert_eq!(
        read.foreign_lanes,
        vec![
            TaskLaneOwnerRead {
                task_id: "foreign-a".into(),
                worktree_path: "first".into()
            },
            TaskLaneOwnerRead {
                task_id: "foreign-b".into(),
                worktree_path: String::new()
            },
        ]
    );
    assert!(matches!(
        database.task_lane_admission_projection("missing", "task_exact"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.task_lane_admission_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.lane.worktree_path',42) WHERE task_id='foreign-a'",[]).unwrap();
    assert!(database
        .task_lane_admission_projection(&plan.id, "task_exact")
        .is_err());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.execution_intent','non_repository') WHERE task_id='task_exact'",[]).unwrap();
    let read = database
        .task_lane_admission_projection(&plan.id, "task_exact")
        .unwrap();
    assert_eq!(read.execution_intent, TaskExecutionIntent::NonRepository);
    assert!(read.foreign_lanes.is_empty());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.execution_intent','invalid') WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .task_lane_admission_projection(&plan.id, "task_exact")
        .is_err());
}

#[test]
fn lane_admission_fixture_ignores_unrequested_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].lane.worktree_path = Some("own".into());
        plan.tasks[1].lane.worktree_path = Some("foreign".into());
        let original = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = original.clone();
            old.id = format!("closed-{index}");
            old.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(old);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .task_lane_admission_projection(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(read.foreign_lanes.len(), 1);
        assert_eq!(read.foreign_lanes[0].task_id, "task_0001");
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=lane_admission tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
