use super::*;

#[test]
fn startup_owner_epoch_preserves_binding_and_full_unsigned_epoch_checks() {
    for variant in 0..12 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.tasks[0].execution_attempts[0].interruption.owner_epoch = 7;
        match variant {
            1 => plan.tasks[0].assigned_tab_id = None,
            2 => plan.tasks[0].interruption.owner_tab_id = Some("other".into()),
            3 => plan.tasks[0].interruption.owner_epoch = 0,
            4 => plan.tasks[0].execution_attempts[0].interruption.owner_epoch = 6,
            5 => plan.tasks[0].execution_attempts[0].closed_at = Some(2),
            6 => plan.tasks[0].execution_attempts[0].retired_at = Some(2),
            7 => plan.status = PlanStatus::Archived,
            8 => {
                plan.tasks[0].interruption.owner_epoch = u64::MAX;
                plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX;
            }
            9 => {
                plan.tasks[0].interruption.owner_epoch = u64::MAX;
                plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX - 1;
            }
            10 => {
                let mut first = plan.tasks[0].execution_attempts[0].clone();
                first.interruption.owner_epoch = 6;
                plan.tasks[0].execution_attempts.insert(0, first);
            }
            11 => plan.tasks[0].status = TaskStatus::Done,
            _ => {}
        }
        let expected = if matches!(variant, 0 | 11) {
            Some(7)
        } else if variant == 8 {
            Some(u64::MAX)
        } else {
            None
        };
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database
                .startup_owner_epoch_projection(&plan.id, "NAMED", "current", "worker")
                .unwrap(),
            expected,
            "variant {variant}"
        );
        assert_eq!(
            database
                .startup_owner_epoch_projection("wrong-plan", "NAMED", "current", "worker")
                .unwrap(),
            None
        );
    }
}

#[test]
fn lifecycle_provider_and_lane_reads_keep_first_match_fallback_and_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].execution_attempts[0].agent_type = None;
    plan.tasks[0].execution_attempts[0].provider = Some("first".into());
    let mut duplicate = plan.tasks[0].execution_attempts[0].clone();
    duplicate.agent_type = Some("second".into());
    plan.tasks[0].execution_attempts.push(duplicate);
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .lifecycle_provider_projection(&plan.id, "NAMED", "current")
            .unwrap()
            .as_deref(),
        Some("first")
    );
    assert_eq!(
        database
            .lifecycle_provider_projection("missing", "NAMED", "current")
            .unwrap(),
        None
    );
    assert_eq!(
        database
            .lifecycle_provider_projection(&plan.id, "NAMED", "missing")
            .unwrap(),
        None
    );
    assert_eq!(
        database
            .task_lane_metadata_projection(&plan.id, "NAMED")
            .unwrap(),
        plan.tasks[0].lane
    );
    assert!(matches!(
        database.task_lane_metadata_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.provider',42) WHERE position=0",[]).unwrap();
    assert!(database
        .lifecycle_provider_projection(&plan.id, "NAMED", "current")
        .is_err());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch','invalid')",[]).unwrap();
    assert!(database
        .startup_owner_epoch_projection(&plan.id, "NAMED", "current", "worker")
        .is_err());
}

#[test]
fn deadline_fixture_returns_only_epoch_provider_and_lane_across_growth() {
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
        plan.tasks[0].assigned_tab_id = Some("deadline-worker".into());
        plan.tasks[0].interruption.owner_tab_id = Some("deadline-worker".into());
        plan.tasks[0].interruption.owner_epoch = u64::MAX;
        plan.tasks[0].execution_attempts[0].executor_tab_id = "deadline-worker".into();
        plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX;
        plan.tasks[0].execution_attempts[0].agent_type = Some("opaque-provider".into());
        let current = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| {
                let mut old = current.clone();
                old.id = format!("old-{index}");
                old.closed_at = Some(2);
                old
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
        let epoch = database
            .startup_owner_epoch_projection(&plan.id, "task_0000", &current.id, "deadline-worker")
            .unwrap();
        assert_eq!(epoch, Some(u64::MAX));
        let provider = database
            .lifecycle_provider_projection(&plan.id, "task_0000", &current.id)
            .unwrap();
        assert_eq!(provider.as_deref(), Some("opaque-provider"));
        let lane = database
            .task_lane_metadata_projection(&plan.id, "task_0000")
            .unwrap();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(epoch, provider, lane)).unwrap().len();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=deadline_metadata tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
