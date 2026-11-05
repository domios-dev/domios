use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn remote_respawn_selects_first_remote_match_after_local_duplicate_and_propagates_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    let mut remote = plan.tasks[0].execution_attempts[0].clone();
    remote.mesh_peer = Some("peer-first".into());
    remote.worktree_path = Some("/remote-source".into());
    plan.tasks[0].execution_attempts.push(remote.clone());
    let mut later = remote.clone();
    later.mesh_peer = Some("peer-later".into());
    plan.tasks[0].execution_attempts.push(later);
    let mut local = plan.tasks[0].execution_attempts[0].clone();
    local.id = "local-only".into();
    plan.tasks[0].execution_attempts.push(local);
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .respawn_remote_source_projection(&plan.id, "task_0000", "current")
            .unwrap(),
        Some(remote)
    );
    assert!(database
        .respawn_remote_source_projection(&plan.id, "task_0000", "missing")
        .unwrap()
        .is_none());
    assert!(database
        .respawn_remote_source_projection(&plan.id, "task_0000", "local-only")
        .unwrap()
        .is_none());
    assert!(matches!(
        database.respawn_remote_source_projection("missing", "task_0000", "current"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.respawn_remote_source_projection(&plan.id, "missing", "current"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.mesh_peer',42) WHERE task_id='task_0000' AND position=1",[]).unwrap();
    assert!(database
        .respawn_remote_source_projection(&plan.id, "task_0000", "current")
        .is_err());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.acceptance_criteria',json('null')) WHERE task_id='task_0000'",[]).unwrap();
    assert!(database
        .task_respawn_acceptance_projection(&plan.id, "task_0000")
        .is_err());
}

fn fixture(consumer: &str) {
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
        plan.tasks[0].execution_attempts[0].mesh_peer = Some("peer".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| TaskExecutionAttempt {
                id: format!("closed-{index}"),
                closed_at: Some(2),
                ..original.clone()
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let source = database
            .respawn_remote_source_projection(&plan.id, "task_0000", "current")
            .unwrap()
            .unwrap();
        assert_eq!(source, original);
        let _initial = database
            .task_launch_projection(&plan.id, "task_0000", None)
            .unwrap();
        let launch = database
            .task_launch_projection(&plan.id, "task_0000", Some("current"))
            .unwrap();
        assert!(launch.source_attempt_exists);
        let (criteria, predecessors, canonical, owner) = if consumer == "task_respawn_http" {
            let metadata = database.document_metadata(&plan.id).unwrap();
            let owner = database.plan_owner_tab_projection(&plan.id).unwrap();
            let criteria = database
                .task_respawn_acceptance_projection(&plan.id, "task_0000")
                .unwrap();
            (criteria, Vec::<String>::new(), metadata.project_path, owner)
        } else {
            let seed = database
                .task_dispatch_seed_projection(&plan.id, "task_0000", "")
                .unwrap();
            let safety = database
                .competition_lane_projection(&plan.id, "task_0000")
                .unwrap();
            (
                seed.acceptance_criteria,
                seed.predecessor_tabs,
                safety.canonical_path,
                safety.owner_tab_id,
            )
        };
        database.review_gate_projection(&plan.id).unwrap();
        assert_eq!(criteria, plan.tasks[0].acceptance_criteria);
        assert!(predecessors.is_empty());
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response =
            serde_json::to_vec(&(source, launch, criteria, predecessors, canonical, owner))
                .unwrap()
                .len();
        assert_eq!(
            metrics.queries,
            if consumer == "task_respawn_http" {
                7
            } else {
                6
            }
        );
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer={consumer} tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn task_respawn_http_fixture_selects_remote_source_without_serialization_or_writes() {
    fixture("task_respawn_http");
}

#[test]
fn respawn_rpc_fixture_selects_remote_source_without_serialization_or_writes() {
    fixture("respawn_rpc");
}
