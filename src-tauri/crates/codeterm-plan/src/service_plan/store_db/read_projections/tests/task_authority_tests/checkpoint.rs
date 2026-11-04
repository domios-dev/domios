use super::*;
use crate::plan::InterruptionCheckpoint;

fn checkpoint() -> InterruptionCheckpoint {
    InterruptionCheckpoint {
        task_id: "task_exact".into(),
        attempt_id: "current".into(),
        tab_id: "worker".into(),
        session_id: "session".into(),
        owner_epoch: 7,
        repo_path: "repo".into(),
        worktree_path: "lane".into(),
        base_head: "base".into(),
        current_head: "current".into(),
        git_status: "opaque status".into(),
        diff_artifact: "opaque artifact".into(),
        diff_sha256: "digest".into(),
        completed_tests: vec!["opaque test".into()],
        unresolved_work: "opaque work".into(),
        next_acceptance_gate: "opaque gate".into(),
    }
}

#[test]
fn checkpoint_adoption_selects_exact_payload_and_handles_partial_interruption_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    assert!(database
        .task_checkpoint_adoption_projection("task_exact")
        .unwrap()
        .checkpoint
        .is_none());
    plan.tasks[0].interruption.checkpoint = Some(checkpoint());
    database.persist_plan_rows(&plan, None).unwrap();
    let selected = database
        .task_checkpoint_adoption_projection("NAMED")
        .unwrap();
    assert_eq!(selected.plan_id, plan.id);
    assert_eq!(selected.checkpoint, Some(checkpoint()));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption',json(?1)) WHERE task_id='task_exact'",[r#"{"owner_epoch":8,"handoff_required":true}"#]).unwrap();
    assert!(database
        .task_checkpoint_adoption_projection("task_exact")
        .unwrap()
        .checkpoint
        .is_none());
    assert!(matches!(
        database.task_checkpoint_adoption_projection("missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.checkpoint',json(?1)) WHERE task_id='task_exact'",[r#"{"task_id":"task_exact"}"#]).unwrap();
    assert!(database
        .task_checkpoint_adoption_projection("task_exact")
        .is_err());
}

#[test]
fn checkpoint_adoption_bytes_ignore_unselected_bodies_and_history() {
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
        let mut checkpoint = checkpoint();
        checkpoint.task_id = "task_0000".into();
        plan.tasks[0].interruption.checkpoint = Some(checkpoint);
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
            .task_checkpoint_adoption_projection("task_0000")
            .unwrap();
        assert_eq!(read.checkpoint, plan.tasks[0].interruption.checkpoint);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=checkpoint_adoption tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
