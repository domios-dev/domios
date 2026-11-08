use super::*;
use crate::plan::CuratorRound;

#[test]
fn identity_materialization_excludes_task_and_curator_history_and_propagates_errors() {
    let mut prior = None;
    for (strides, attempts, extra_body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.owner.tab_id = Some("owner".into());
        plan.owner.session_id = Some("session".into());
        plan.team_id = Some("team".into());
        plan.tasks[0].body.push_str(&"body".repeat(extra_body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| CuratorRound {
                    number,
                    summary: "history".repeat(128),
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.plan_identity_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(read.owner, plan.owner);
        assert_eq!(read.team_id, plan.team_id);
        assert_eq!(metrics.queries, 1);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response_bytes = serde_json::to_vec(&read).unwrap().len();
        println!("fixture={} consumer=plan_identity tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0", fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id=?1",[&plan.id]).unwrap();
        assert!(matches!(
            database.plan_identity_projection(&plan.id),
            Err(PlanError::Quarantined { .. })
        ));
        assert!(matches!(
            database.plan_identity_projection("missing"),
            Err(PlanError::NotFound)
        ));
    }
}

#[test]
fn identity_owner_and_team_are_one_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Identity".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.owner.tab_id = Some("a".into());
    plan.team_id = Some("a".into());
    let database = fixtures::database(directory.path(), &plan);
    let path = directory.path().join("plans.db");
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for i in 0..16 {
            let transaction = connection.transaction().unwrap();
            let value = if i % 2 == 0 { "b" } else { "a" };
            transaction.execute("UPDATE plans SET team_id=?2,header_json=json_set(header_json,'$.owner.tab_id',?2,'$.mode',CASE WHEN ?2='a' THEN 'planning' ELSE 'paused' END) WHERE id=?1",params![id,value]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let read = database.plan_identity_projection(&plan.id).unwrap();
        assert_eq!(read.owner.tab_id, read.team_id);
        let target = database
            .notice_delivery_target_projection(&plan.id)
            .unwrap();
        assert_eq!(
            target.mode == crate::plan::PlanMode::Paused,
            target.owner_tab_id.as_deref() == Some("b")
        );
        barrier.wait();
    }
    writer.join().unwrap();
}

#[test]
fn notice_delivery_target_is_bounded_and_selected_failures_are_loud() {
    let mut previous = None;
    for (strides, attempts, body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.owner.tab_id = Some("owner".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| CuratorRound {
                    number,
                    summary: "history".repeat(128),
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.owner.title',?2) WHERE id=?1",
                params![plan.id, "unused".repeat(body)],
            )
            .unwrap();
        let before = fixtures::audit_counts(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let target = database
            .notice_delivery_target_projection(&plan.id)
            .unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(target.owner_tab_id.as_deref(), Some("owner"));
        assert_eq!(target.mode, plan.mode);
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 512);
        assert!(decoded < 512);
        assert_eq!(serialized, 0);
        assert_eq!(fixtures::audit_counts(&database), before);
        let response = serde_json::to_vec(&target).unwrap().len();
        println!("fixture={} consumer=notice_delivery_target tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
        for mode in ["planning", "approved", "execution", "paused", "complete"] {
            database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.mode',?2,'$.owner.tab_id',NULL) WHERE id=?1",params![plan.id,mode]).unwrap();
            let target = database
                .notice_delivery_target_projection(&plan.id)
                .unwrap();
            assert_eq!(serde_json::to_value(target.mode).unwrap(), json!(mode));
            assert!(target.owner_tab_id.is_none());
        }
        for path in ["$.mode", "$.owner.tab_id", "$.owner"] {
            database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.mode','planning','$.owner',json('{\"tab_id\":\"owner\"}')) WHERE id=?1",[&plan.id]).unwrap();
            database
                .connection
                .lock()
                .execute(
                    "UPDATE plans SET header_json=json_set(header_json,?2,17) WHERE id=?1",
                    params![plan.id, path],
                )
                .unwrap();
            assert!(matches!(
                database.notice_delivery_target_projection(&plan.id),
                Err(PlanError::Quarantined { .. })
            ));
        }
        assert!(matches!(
            database.notice_delivery_target_projection("missing"),
            Err(PlanError::NotFound)
        ));
    }
}
