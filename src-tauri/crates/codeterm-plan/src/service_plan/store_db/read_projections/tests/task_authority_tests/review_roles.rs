use super::*;

#[test]
fn review_roles_preserve_status_assignment_and_current_skill_receipt_rules() {
    for variant in 0..12 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.tasks[0].status = TaskStatus::InProgress;
        match variant {
            1 => plan.tasks[0].assigned_tab_id = None,
            2 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].closed_at = Some(1);
            }
            3 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].retired_at = Some(1);
            }
            4 => plan.tasks[0].status = TaskStatus::Done,
            5 => plan.tasks[0].status = TaskStatus::Cancelled,
            6 => plan.tasks[0].status = TaskStatus::Paused,
            7 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].executor_tab_id = "other".into();
            }
            8 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].reported_at = Some(1);
            }
            9 => plan.tasks[0].assigned_tab_id = Some("\u{2003}".into()),
            10 => plan.status = PlanStatus::Archived,
            11 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].session_binding_nonce = None;
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let expected = plan.tasks.iter().any(|task| {
            !task.status.is_terminal()
                && (task.assigned_tab_id.as_deref() == Some("worker")
                    || task.execution_attempts.iter().any(|attempt| {
                        attempt.executor_tab_id == "worker"
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    }))
        });
        for team in [None, Some(" "), Some("team")] {
            assert_eq!(
                database
                    .plan_skill_receipt_projection(&plan.id, team, "worker")
                    .unwrap(),
                expected,
                "variant {variant}"
            );
        }
        assert_eq!(
            database.task_status_projection(&plan.id, "NAMED").unwrap(),
            plan.tasks[0].status
        );
        assert_eq!(
            database
                .task_assignment_projection(&plan.id, "task_exact")
                .unwrap(),
            plan.tasks[0].assigned_tab_id
        );
        let mut other = plan.clone();
        other.id = "other-plan".into();
        other.status = PlanStatus::Active;
        other.team_id = Some("team".into());
        database.persist_plan_rows(&other, None).unwrap();
        assert!(!database
            .plan_skill_receipt_projection(&plan.id, Some("\u{2003}team\u{2009}"), "worker")
            .unwrap());
        assert!(!database
            .plan_skill_receipt_projection("missing", None, "worker")
            .unwrap());
        assert!(database
            .task_assignment_projection(&plan.id, "missing")
            .unwrap()
            .is_none());
    }
}

#[test]
fn review_roles_fixture_returns_only_selected_facts_and_errors_stay_loud() {
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
        let status = database
            .task_status_projection(&plan.id, "task_0000")
            .unwrap();
        let assignment = database
            .task_assignment_projection(&plan.id, "task_0000")
            .unwrap();
        let receipt = database
            .plan_skill_receipt_projection(&plan.id, None, "worker")
            .unwrap();
        assert_eq!(assignment.as_deref(), Some("worker"));
        assert!(receipt);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(status, assignment, receipt))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=review_roles tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.assigned_tab_id',42) WHERE task_id='task_0000'",[]).unwrap();
        assert!(database
            .task_assignment_projection(&plan.id, "task_0000")
            .is_err());
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status','invalid') WHERE task_id='task_0000'",[]).unwrap();
        assert!(database
            .task_status_projection(&plan.id, "task_0000")
            .is_err());
        assert!(database
            .plan_skill_receipt_projection(&plan.id, None, "worker")
            .is_err());
    }
}
