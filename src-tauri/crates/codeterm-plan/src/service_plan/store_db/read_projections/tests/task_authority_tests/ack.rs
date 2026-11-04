use super::*;

#[test]
fn ack_projection_matches_startup_subject_and_selects_only_retained_response() {
    for variant in 0..20 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.tasks[0].status = TaskStatus::Pending;
        match variant {
            1 => plan.tasks[0].execution_attempts[0].acknowledged_at = Some(0),
            2 => plan.tasks[0].execution_attempts[0].acknowledged_at = Some(u64::MAX),
            3 => plan.tasks[0].assigned_tab_id = Some("other".into()),
            4 => plan.tasks[0].execution_attempts[0].closed_at = Some(1),
            5 => plan.tasks[0].execution_attempts[0].retired_at = Some(1),
            6 => plan.tasks[0].execution_attempts[0].session_id = None,
            7 => plan.tasks[0].execution_attempts[0].session_binding_nonce = None,
            8..=13 => {
                plan.tasks[0].execution_attempts[0].competition_group_id = Some("group".into());
                match variant {
                    9 => plan.tasks[0].execution_attempts[0].session_id = Some("\u{2003}".into()),
                    10 => plan.tasks[0].execution_attempts[0].session_id = None,
                    11..=13 => {
                        let mut later = plan.tasks[0].execution_attempts[0].clone();
                        later.id = "later".into();
                        later.competition_group_id = Some("new-group".into());
                        if variant == 12 {
                            later.retired_at = Some(1);
                        }
                        if variant == 13 {
                            later.closed_at = Some(1);
                        }
                        plan.tasks[0].execution_attempts.push(later);
                    }
                    _ => {}
                }
            }
            14 | 15 => {
                let mut later = plan.tasks[0].execution_attempts[0].clone();
                later.id = "later".into();
                if variant == 15 {
                    later.closed_at = Some(1);
                }
                plan.tasks[0].execution_attempts.push(later);
            }
            16 => {
                let mut first = plan.tasks[0].execution_attempts[0].clone();
                first.session_binding_nonce = Some("first-nonce".into());
                plan.tasks[0].execution_attempts.insert(0, first);
            }
            17 => plan.status = PlanStatus::Archived,
            18 => plan.tasks[0].status = TaskStatus::InReview,
            19 => {
                plan.tasks[0].status = TaskStatus::Done;
                plan.tasks[0].execution_attempts[0].acknowledged_at = Some(1);
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let read = database
            .task_ack_authority_projection(&plan.id, "NAMED", "worker", "current", None, false)
            .unwrap();
        let actual = read.current("worker").map(|(attempt, competitive)| {
            (
                attempt.id.clone(),
                competitive,
                attempt.session.clone(),
                attempt.nonce.clone(),
                attempt.acknowledged,
            )
        });
        let expected = crate::service_plan::task_dispatch::startup_attempt(
            &plan.tasks[0],
            "worker",
            "current",
        )
        .map(|(attempt, competitive)| {
            (
                attempt.id.clone(),
                competitive,
                attempt.session_id.clone(),
                attempt.session_binding_nonce.clone(),
                attempt.acknowledged_at.is_some(),
            )
        });
        assert_eq!(actual, expected, "variant {variant}");
        assert_eq!(read.task_id, "task_exact");
        assert_eq!(read.status, plan.tasks[0].status);
        let response = crate::service_plan::task_dispatch::startup_attempt(
            &plan.tasks[0],
            "worker",
            "current",
        )
        .map(|(attempt, _)| attempt)
        .filter(|attempt| attempt.acknowledged_at.is_some());
        assert_eq!(
            read.response_attempt.as_ref(),
            response,
            "variant {variant}"
        );
        assert!(matches!(
            database.task_ack_authority_projection(
                "missing",
                "task_exact",
                "worker",
                "current",
                None,
                false
            ),
            Err(PlanError::NotFound)
        ));
        assert!(matches!(
            database.task_ack_authority_projection(
                &plan.id, "missing", "worker", "current", None, false
            ),
            Err(PlanError::TaskNotFound)
        ));
    }
}

#[test]
fn ack_response_is_materialized_only_after_the_exact_runtime_binding_verdict() {
    use crate::service_plan::task_authority_projection::acknowledging_binding_matches;
    for session in [None, Some("session")] {
        for nonce in [None, Some("nonce")] {
            let directory = tempfile::tempdir().unwrap();
            let mut plan = authority_plan(directory.path());
            let attempt = &mut plan.tasks[0].execution_attempts[0];
            attempt.session_id = session.map(str::to_string);
            attempt.session_binding_nonce = nonce.map(str::to_string);
            attempt.acknowledged_at = Some(1);
            let database = fixtures::database(directory.path(), &plan);
            for runtime in [
                None,
                Some(("session", "nonce")),
                Some(("wrong", "nonce")),
                Some(("session", "wrong")),
            ] {
                for enforce in [false, true] {
                    let read = database
                        .task_ack_authority_projection(
                            &plan.id,
                            "task_exact",
                            "worker",
                            "current",
                            runtime,
                            enforce,
                        )
                        .unwrap();
                    assert_eq!(
                        read.response_attempt.is_some(),
                        acknowledging_binding_matches(session, nonce, runtime, enforce)
                    );
                }
            }
        }
    }
}

#[test]
fn ack_projection_fixture_keeps_authority_and_selected_response_bounded() {
    for retained in [false, true] {
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
            if retained {
                plan.tasks[0].execution_attempts[0].acknowledged_at = Some(1);
            }
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
                .task_ack_authority_projection(
                    &plan.id,
                    "task_0000",
                    "worker",
                    "current",
                    None,
                    false,
                )
                .unwrap();
            assert!(read.current("worker").is_some());
            assert_eq!(read.response_attempt.is_some(), retained);
            let metrics = take_metrics();
            let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
            let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
            let response = serde_json::to_vec(&read).unwrap().len();
            assert_eq!(metrics.queries, 1);
            assert!(metrics.materialized_bytes < 4096);
            assert!(decoded < 4096);
            assert_eq!(serialized, 0);
            assert!(fixtures::audit_counts(&database).is_empty());
            println!("fixture={} consumer=ack_admission retained={retained} tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
            let measured = (metrics, decoded, response);
            if let Some(prior) = &prior {
                assert_eq!(&measured, prior);
            }
            prior = Some(measured);
            database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',42) WHERE task_id='task_0000' AND attempt_id='current'",[]).unwrap();
            assert!(database
                .task_ack_authority_projection(
                    &plan.id,
                    "task_0000",
                    "worker",
                    "current",
                    None,
                    false
                )
                .is_err());
        }
    }
}
