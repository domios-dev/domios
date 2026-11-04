use super::*;
use crate::plan::{PlanStatus, TaskStatus};

#[test]
fn filtered_index_preserves_raw_project_owner_status_order_and_entry_shape() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = Plan::new(
        "First".into(),
        "raw-A".into(),
        OrchestratorRef::from_tab("owner"),
    );
    first.id = "pl_z".into();
    first.canonical_project_path = "canonical-A".into();
    first.owner.session_id = Some("session".into());
    first.tasks = vec![
        crate::service_plan::load_audit::task("pending", TaskStatus::Pending, Some(" duplicate ")),
        crate::service_plan::load_audit::task("paused", TaskStatus::Paused, Some("duplicate")),
        crate::service_plan::load_audit::task("done", TaskStatus::Done, Some("finished")),
    ];
    let database = fixtures::database(directory.path(), &first);
    let mut other = Plan::new("Other".into(), "raw-B".into(), OrchestratorRef::default());
    other.id = "pl_a".into();
    database.persist_plan(&other, None).unwrap();
    let mut archived = Plan::new(
        "Archived".into(),
        "raw-A".into(),
        OrchestratorRef::default(),
    );
    archived.id = "pl_m".into();
    archived.status = PlanStatus::Archived;
    archived.owner.session_id = Some("session-only".into());
    database.persist_plan(&archived, None).unwrap();
    let mut blank = Plan::new(
        "Blank".into(),
        "raw-A".into(),
        OrchestratorRef::from_tab(""),
    );
    blank.id = "pl_blank".into();
    database.persist_plan(&blank, None).unwrap();
    let mut unowned = Plan::new("Unowned".into(), "raw-A".into(), OrchestratorRef::default());
    unowned.id = "pl_unowned".into();
    database.persist_plan(&unowned, None).unwrap();
    let legacy = database.list_index_entries().unwrap();
    let all = database.plan_list_index(None, None, false, None).unwrap();
    assert_eq!(
        serde_json::to_value(&all).unwrap(),
        serde_json::to_value(&legacy).unwrap()
    );
    assert_eq!(
        all.iter()
            .map(|entry| entry.plan_id.as_str())
            .collect::<Vec<_>>(),
        vec!["pl_z", "pl_a", "pl_m", "pl_blank", "pl_unowned"]
    );
    assert_eq!(all[0].assigned_tab_ids, vec!["duplicate", "duplicate"]);
    let ids = |project, status, unowned, owner| {
        database
            .plan_list_index(project, status, unowned, owner)
            .unwrap()
            .into_iter()
            .map(|entry| entry.plan_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(Some("raw-A"), None, false, None),
        vec!["pl_z", "pl_m", "pl_blank", "pl_unowned"]
    );
    assert!(ids(Some("canonical-A"), None, false, None).is_empty());
    assert_eq!(
        ids(None, Some(PlanStatus::Archived), false, None),
        vec!["pl_m"]
    );
    assert_eq!(ids(Some("raw-A"), None, true, None), vec!["pl_unowned"]);
    assert_eq!(ids(None, None, false, Some("session")), vec!["pl_z"]);
    assert_eq!(ids(None, None, false, Some("session-only")), vec!["pl_m"]);
    assert_eq!(ids(None, None, false, Some("")), vec!["pl_blank"]);
    assert!(ids(None, None, true, Some("owner")).is_empty());
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id='pl_a'",[]).unwrap();
    assert_eq!(
        ids(Some("raw-A"), None, false, None),
        vec!["pl_z", "pl_m", "pl_blank", "pl_unowned"]
    );
    assert!(database
        .plan_list_index(Some("raw-B"), None, false, None)
        .is_err());
}

#[test]
fn filtered_index_materializes_no_unmatched_rows_or_assignments() {
    let mut prior = None;
    for (strides, attempts, extra_tasks, extra_body) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 4000, 32768),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut other = fixtures::fixture(directory.path(), strides, attempts);
        other.project_path = "other".into();
        other.tasks[0].body.push_str(&"x".repeat(extra_body));
        for index in 0..extra_tasks {
            other.tasks.push(crate::service_plan::load_audit::task(
                &format!("extra-{index}"),
                TaskStatus::Pending,
                Some("unmatched-worker"),
            ));
        }
        let database = fixtures::database(directory.path(), &other);
        let mut matched = Plan::new(
            "Matched".into(),
            "wanted".into(),
            OrchestratorRef::from_tab("owner"),
        );
        matched.id = "pl_match".into();
        matched.tasks = vec![crate::service_plan::load_audit::task(
            "selected",
            TaskStatus::Pending,
            Some("matched-worker"),
        )];
        database.persist_plan(&matched, None).unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let rows = database
            .plan_list_index(Some("wanted"), None, false, None)
            .unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].plan_id, "pl_match");
        assert_eq!(rows[0].assigned_tab_ids, vec!["matched-worker"]);
        assert_eq!(metrics.queries, 2);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response_bytes = serde_json::to_vec(&rows).unwrap().len();
        println!("fixture={} consumer=filtered_plan_index tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_tasks={extra_tasks} extra_body={extra_body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,other.tasks.len()+1,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn filtered_index_rows_and_assignment_metadata_are_one_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "worker-a".into(),
        "wanted".into(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "one",
        TaskStatus::Pending,
        Some("worker-a"),
    )];
    let database = fixtures::database(directory.path(), &plan);
    let id = plan.id.clone();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let value = if index % 2 == 0 {
                "worker-b"
            } else {
                "worker-a"
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET title=?2 WHERE id=?1", params![id, value])
                .unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.assigned_tab_id',?2) WHERE plan_id=?1",params![id,value]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let rows = database
            .plan_list_index(Some("wanted"), None, false, None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].assigned_tab_ids, vec![rows[0].title.clone()]);
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute("UPDATE plans SET status='unknown' WHERE id=?1", [&plan.id])
        .unwrap();
    assert!(matches!(
        database.plan_list_index(Some("wanted"), None, false, None),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn selected_index_metadata_errors_are_loud_and_unmatched_headers_stay_unread() {
    let directory = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "Selected".into(),
        "wanted".into(),
        OrchestratorRef::default(),
    );
    let database = fixtures::database(directory.path(), &plan);
    for owner in [
        json!(42),
        json!({"tab_id":42}),
        json!({"session_id":[]}),
        json!({"title":false}),
    ] {
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.owner',json(?2)) WHERE id=?1",
                params![plan.id, owner.to_string()],
            )
            .unwrap();
        for (unowned, owner) in [(false, None), (true, None), (false, Some("42"))] {
            assert!(database
                .plan_list_index(Some("wanted"), None, unowned, owner)
                .is_err());
            assert!(database
                .plan_list_index(Some("unmatched"), None, unowned, owner)
                .unwrap()
                .is_empty());
        }
    }
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner',json('{}')) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert_eq!(
        database
            .plan_list_index(Some("wanted"), None, true, None)
            .unwrap()
            .len(),
        1
    );
    for (path, value) in [
        ("$.canonical_project_path", json!(null)),
        ("$.loops", json!(42)),
        ("$.goal", json!([])),
        ("$.orphaned", json!("invalid")),
    ] {
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(header_json,?2,json(?3)) WHERE id=?1",
                params![plan.id, path, value.to_string()],
            )
            .unwrap();
        assert!(database
            .plan_list_index(Some("wanted"), None, false, None)
            .is_err());
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_remove(header_json,?2) WHERE id=?1",
                params![plan.id, path],
            )
            .unwrap();
    }
    database
        .connection
        .lock()
        .execute("UPDATE plans SET status='unknown' WHERE id=?1", [&plan.id])
        .unwrap();
    assert!(matches!(
        database.load_index_row(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
}
