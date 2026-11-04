use super::*;
use crate::plan::WorkerPlacement;
use crate::service_plan::{CanonicalWriterHoldRead, LaneIntegrationRead};

fn integration_reference(plan: &Plan, task_id: &str) -> Option<LaneIntegrationRead> {
    let task = plan.tasks.iter().find(|task| task.id == task_id)?;
    let worktree_path = task
        .lane
        .worktree_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())?;
    let (holder, attempt) = plan.tasks.iter().find_map(|task| {
        task.execution_attempts
            .iter()
            .find(|attempt| {
                attempt.closed_at.is_none()
                    && attempt.retired_at.is_none()
                    && !attempt.placement.is_isolated_lane()
            })
            .map(|attempt| (task, attempt))
    })?;
    if holder.id == task.id {
        return None;
    }
    Some(LaneIntegrationRead {
        plan_id: plan.id.clone(),
        task_id: task.id.clone(),
        worktree_path: worktree_path.into(),
        canonical_path: if plan.canonical_project_path.trim().is_empty() {
            plan.project_path.clone()
        } else {
            plan.canonical_project_path.clone()
        },
        holder: CanonicalWriterHoldRead {
            task_id: holder.id.clone(),
            attempt_id: attempt.id.clone(),
            placement: attempt.placement,
        },
    })
}

fn configure(plan: &mut Plan) {
    plan.tasks[0].lane.worktree_path = Some("lane".into());
    let mut holder = plan.tasks[0].clone();
    holder.id = "holder".into();
    holder.execution_attempts[0].id = "holder-attempt".into();
    holder.execution_attempts[0].placement = WorkerPlacement::CanonicalWriter;
    plan.tasks.push(holder);
}

#[test]
fn review_followup_preserves_latest_local_worker_and_first_canonical_hold() {
    for variant in 0..15 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        configure(&mut plan);
        match variant {
            1 => plan.tasks[1].execution_attempts[0].closed_at = Some(1),
            2 => plan.tasks[1].execution_attempts[0].retired_at = Some(1),
            3 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::IsolatedLane,
            4 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::ReadOnly,
            5 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::NonRepository,
            6 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::Shared,
            7 => plan.tasks[0].lane.worktree_path = None,
            8 => plan.tasks[0].lane.worktree_path = Some("\u{2003}".into()),
            9 => plan.tasks[0].execution_attempts[0].placement = WorkerPlacement::Shared,
            10 => plan.tasks[0].execution_attempts[0].mesh_peer = Some("peer".into()),
            11 => plan.canonical_project_path = "\u{2009}\t".into(),
            12 => plan.canonical_project_path = " canonical ".into(),
            13 => {
                let mut first = plan.tasks[1].execution_attempts[0].clone();
                first.id = "isolated".into();
                first.placement = WorkerPlacement::IsolatedLane;
                plan.tasks[1].execution_attempts.insert(0, first);
            }
            14 => {
                let mut last = plan.tasks[0].execution_attempts[0].clone();
                last.executor_tab_id = "last".into();
                last.closed_at = Some(1);
                last.retired_at = Some(2);
                plan.tasks[0].execution_attempts.push(last);
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database
                .task_lane_integration_projection(&plan.id, "task_exact")
                .unwrap(),
            integration_reference(&plan, "task_exact"),
            "variant {variant}"
        );
        let worker = plan.tasks[0].execution_attempts.last().and_then(|attempt| {
            attempt
                .mesh_peer
                .is_none()
                .then(|| attempt.executor_tab_id.clone())
        });
        assert_eq!(
            database
                .task_review_worker_projection(&plan.id, "task_exact")
                .unwrap(),
            worker,
            "variant {variant}"
        );
        assert!(database
            .task_lane_integration_projection(&plan.id, "missing")
            .unwrap()
            .is_none());
        assert!(database
            .task_review_worker_projection("missing", "task_exact")
            .unwrap()
            .is_none());
    }
}

#[test]
fn review_followup_fixture_returns_only_selected_worker_lane_and_holder() {
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
        plan.tasks[0].lane.worktree_path = Some("lane".into());
        plan.tasks[2].execution_attempts = vec![plan.tasks[0].execution_attempts[0].clone()];
        plan.tasks[2].execution_attempts[0].placement = WorkerPlacement::CanonicalWriter;
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| {
                let mut attempt = original.clone();
                attempt.id = format!("closed-{index}");
                attempt.closed_at = Some(2);
                attempt
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let integration = database
            .task_lane_integration_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        let worker = database
            .task_review_worker_projection(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(integration.holder.task_id, "task_0002");
        assert_eq!(worker.as_deref(), Some("worker"));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(integration, worker)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=review_followup tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.placement','invalid') WHERE task_id='task_0002'",[]).unwrap();
        assert!(database
            .task_lane_integration_projection(&plan.id, "task_0000")
            .is_err());
    }
}
