use super::*;
use crate::plan::{ControlLoopMode, PlanMode, TaskStatus};
use crate::service_plan::state::task_projection;
use sha2::{Digest, Sha256};

fn original_state_key(plan: &Plan) -> String {
    let tasks = plan
        .tasks
        .iter()
        .map(|task| {
            let projection = task_projection(plan, task);
            let waits = projection
                .dependency_waits
                .iter()
                .map(|wait| {
                    format!(
                        "{}:{}",
                        wait.task_id,
                        wait.status
                            .as_ref()
                            .map(TaskStatus::key)
                            .unwrap_or("missing")
                    )
                })
                .collect::<Vec<_>>()
                .join("|");
            format!(
                "{}:{:?}:{:?}:{waits}",
                task.id, projection.presentation, task.status_label
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "owner:{}:Orchestrator:{:?}:{:?}:{tasks}",
        plan.id, plan.status, plan.mode
    )
}

#[test]
fn reminder_reference_keeps_decisions_and_digest_with_payload_independent_reads() {
    let mut baseline = None;
    for body in [0, 32768] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), 0, 1);
        plan.mode = PlanMode::Execution;
        plan.control_loop.mode = ControlLoopMode::Heartbeat;
        plan.control_loop.heartbeat_interval_secs = Some(27);
        plan.owner.tab_id = Some("owner".into());
        plan.tasks[0].status = TaskStatus::Pending;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].status_label = Some(format!("escaped \" label\n{}", "x".repeat(body)));
        plan.tasks[0].body.push_str(&"y".repeat(body));
        plan.tasks[0].depends_on = vec![plan.tasks[1].id.clone(), "missing".into()];
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database
            .reminder_read_snapshot(|text| text.chars().take(512).collect())
            .unwrap();
        let candidates = snapshot.candidate_reference(Some("owner")).unwrap();
        assert_eq!(candidates.len(), 1);
        let read = &candidates[0];
        assert_eq!(
            read.state_key_digest,
            format!("{:x}", Sha256::digest(original_state_key(&plan).as_bytes()))
        );
        assert_eq!(read.control_loop.interval(50), 27);
        assert!(read.control_loop.owner_heartbeat());
        assert_eq!(
            read.actionable_task_ids(),
            crate::service_plan::state::orchestrator_actionable_task_ids(&plan)
        );
        for (task, original) in read.tasks.iter().zip(&plan.tasks) {
            let expected = task_projection(&plan, original);
            assert_eq!(task.projection.presentation, expected.presentation);
            assert_eq!(
                task.projection.actionable,
                expected.needs_orchestrator_action()
            );
            let selected = task
                .projection
                .dependency_waits
                .iter()
                .map(|wait| (&wait.task_id, &wait.status, wait.missing))
                .collect::<Vec<_>>();
            let original = expected
                .dependency_waits
                .iter()
                .map(|wait| (&wait.task_id, &wait.status, wait.missing))
                .collect::<Vec<_>>();
            assert_eq!(selected, original);
        }
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert!(decoded > 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&candidates).unwrap().len();
        println!("fixture={} consumers=reminder_reference_task_facts,reminder_reference_state_key tasks={} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(baseline) = &baseline {
            assert_eq!(&measured, baseline);
        }
        baseline = Some(measured);
    }
}

#[test]
fn reminder_snapshot_is_pinned_and_sql_errors_remain_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.mode = PlanMode::Execution;
    let database = fixtures::database(directory.path(), &plan);
    let old = database
        .reminder_read_snapshot(|text| text.chars().take(512).collect())
        .unwrap();
    let before = old.plan(&plan.id).unwrap().unwrap();
    plan.tasks[0].status = TaskStatus::Done;
    plan.tasks[0].status_label = Some("changed".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        old.plan(&plan.id).unwrap().unwrap().state_key_digest,
        before.state_key_digest
    );
    let new = database
        .reminder_read_snapshot(|text| text.chars().take(512).collect())
        .unwrap();
    assert_ne!(
        new.plan(&plan.id).unwrap().unwrap().state_key_digest,
        before.state_key_digest
    );
    assert!(new.plan("missing").unwrap().is_none());
    assert!(new.candidate_reference(Some(" \t ")).unwrap().is_empty());
    drop(old);
    drop(new);
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    let broken = database
        .reminder_read_snapshot(|text| text.chars().take(512).collect())
        .unwrap();
    assert!(broken.plan(&plan.id).is_err());
    assert!(broken.candidate_reference(None).is_err());
}
