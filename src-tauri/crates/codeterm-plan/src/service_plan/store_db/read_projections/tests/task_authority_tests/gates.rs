use super::*;

#[test]
fn gate_claim_projection_matches_the_held_task_rule_including_retired_unclosed_attempts() {
    let directory = tempfile::tempdir().unwrap();
    let mut base = authority_plan(directory.path());
    base.tasks[0].execution_attempts[0].interruption.owner_epoch = 7;
    base.tasks[0].execution_attempts[0].authorized_gates =
        Some(vec!["allowed".into(), "second".into()]);
    let mut variants = vec![base.clone()];
    for field in [
        "unassigned",
        "foreign_assignment",
        "zero_epoch",
        "wrong_attempt_epoch",
        "retired",
        "closed",
        "foreign_executor",
        "no_gates",
        "empty_gates",
        "later_none",
        "later_wrong_epoch",
    ] {
        let mut plan = base.clone();
        let task = &mut plan.tasks[0];
        match field {
            "unassigned" => task.assigned_tab_id = None,
            "foreign_assignment" => task.assigned_tab_id = Some("foreign".into()),
            "zero_epoch" => task.interruption.owner_epoch = 0,
            "wrong_attempt_epoch" => task.execution_attempts[0].interruption.owner_epoch = 8,
            "retired" => task.execution_attempts[0].retired_at = Some(2),
            "closed" => task.execution_attempts[0].closed_at = Some(2),
            "foreign_executor" => task.execution_attempts[0].executor_tab_id = "foreign".into(),
            "no_gates" => task.execution_attempts[0].authorized_gates = None,
            "empty_gates" => task.execution_attempts[0].authorized_gates = Some(vec![]),
            "later_none" | "later_wrong_epoch" => {
                let mut later = task.execution_attempts[0].clone();
                later.id = "later".into();
                later.authorized_gates = None;
                if field == "later_wrong_epoch" {
                    later.interruption.owner_epoch = 8;
                }
                task.execution_attempts.push(later);
            }
            _ => unreachable!(),
        }
        variants.push(plan);
    }
    let database = fixtures::database(directory.path(), &base);
    for (index, plan) in variants.iter().enumerate() {
        database.persist_plan_rows(plan, None).unwrap();
        let read = database
            .worker_gate_claim_projection(&plan.id, "NAMED", "worker")
            .unwrap();
        for gate in ["allowed", "forbidden"] {
            let expected = crate::service_plan::task_attempt_gates::validate_authorized_gate_claim(
                &plan.tasks[0],
                "worker",
                Some(gate),
            )
            .map_err(|error| error.to_string());
            let actual = crate::service_plan::task_attempt_gates::validate_projected_gate_claim(
                read.as_deref(),
                gate,
            )
            .map_err(|error| error.to_string());
            assert_eq!(actual, expected, "variant {index} gate {gate}");
        }
    }
    assert!(matches!(
        database.worker_gate_claim_projection("missing", "task_exact", "worker"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.worker_gate_claim_projection(&base.id, "missing", "worker"),
        Err(PlanError::TaskNotFound)
    ));
    database.persist_plan_rows(&base, None).unwrap();
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.interruption.owner_epoch','invalid') WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .worker_gate_claim_projection(&base.id, "task_exact", "worker")
        .is_err());
    database.persist_plan_rows(&base, None).unwrap();
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.authorized_gates',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .worker_gate_claim_projection(&base.id, "task_exact", "worker")
        .is_err());
}

#[test]
fn gate_claim_bytes_ignore_body_and_closed_history_growth() {
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
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].execution_attempts[0].authorized_gates = Some(vec!["allowed".into()]);
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
            .worker_gate_claim_projection(&plan.id, "task_0000", "worker")
            .unwrap();
        assert_eq!(read, Some(vec!["allowed".into()]));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=worker_gate_claim tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
