use super::*;
use crate::plan::TransferRecord;

#[test]
fn team_mutation_response_excludes_unselected_task_and_owner_history() {
    let mut previous = None;
    for (strides, attempts, body, transfers) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let latest = TransferRecord {
            from: OrchestratorRef::from_tab("before"),
            to: OrchestratorRef::from_tab("after"),
            ts: u64::MAX,
            reason: "selected transfer".into(),
        };
        plan.owner = latest.to.clone();
        plan.team_id = Some("team".into());
        plan.owner_history = (0..transfers)
            .map(|_| TransferRecord {
                reason: "unselected history".repeat(32),
                ..latest.clone()
            })
            .collect();
        plan.owner_history.push(latest.clone());
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.team_plan_mutation_projection(&plan.id).unwrap();
        assert_eq!(read.id, plan.id);
        assert_eq!(read.title, plan.title);
        assert_eq!(read.owner, plan.owner);
        assert_eq!(read.team_id, plan.team_id);
        assert_eq!(read.status, plan.status);
        assert_eq!(read.planner, plan.planner);
        assert_eq!(read.orphaned, plan.orphaned);
        assert_eq!(read.latest_owner_transfer, Some(latest));
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=team_mutation_response tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} old_transfers={transfers} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn team_mutation_response_refuses_malformed_selected_fields_and_unknown_plans() {
    let directory = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "Team response".into(),
        directory.path().display().to_string(),
        Default::default(),
    );
    let database = fixtures::database(directory.path(), &plan);
    assert!(database
        .team_plan_mutation_projection(&plan.id)
        .unwrap()
        .latest_owner_transfer
        .is_none());
    assert!(matches!(
        database.team_plan_mutation_projection("missing"),
        Err(PlanError::NotFound)
    ));
    let header: String = database
        .connection
        .lock()
        .query_row(
            "SELECT header_json FROM plans WHERE id=?1",
            [&plan.id],
            |row| row.get(0),
        )
        .unwrap();
    for (field, value) in [
        ("owner", "17"),
        ("planner", "17"),
        ("orphaned", "17"),
        ("owner_history", "null"),
        ("owner_history", "[null]"),
        (
            "owner_history",
            "[{\"from\":{},\"to\":{},\"ts\":null,\"reason\":\"bad\"}]",
        ),
    ] {
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(?2,?3,json(?4)) WHERE id=?1",
                params![plan.id, header, format!("$.{field}"), value],
            )
            .unwrap();
        assert!(
            matches!(
                database.team_plan_mutation_projection(&plan.id),
                Err(PlanError::Quarantined { .. })
            ),
            "{field}={value}"
        );
    }
}

#[test]
fn team_close_flags_select_only_ordered_reasons_and_propagate_selected_errors() {
    let mut previous = None;
    for (strides, attempts, body, unfinished) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.orphaned = Some(crate::plan::PlanOrphan {
            team_id: "closed".into(),
            reason: "unfinished work".into(),
            at: u64::MAX,
            unfinished_task_ids: (0..unfinished)
                .map(|index| format!("task-{index}"))
                .collect(),
        });
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let flags = database
            .team_close_flags_projection(&["missing".into(), plan.id.clone(), plan.id.clone()])
            .unwrap();
        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0].plan_id, plan.id);
        assert_eq!(flags[0].reason, "unfinished work");
        assert_eq!(flags[1].plan_id, plan.id);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&flags).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 1024);
        assert!(decoded < 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=team_close_flags tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unfinished_ids={unfinished} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
        database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.orphaned.reason',17) WHERE id=?1",[&plan.id]).unwrap();
        assert!(database
            .team_close_flags_projection(&[])
            .unwrap()
            .is_empty());
        assert!(matches!(
            database.team_close_flags_projection(&[plan.id.clone()]),
            Err(PlanError::Quarantined { .. })
        ));
    }
}
