use super::*;

const NOW: u64 = 2_000_000_000_000;

fn database() -> PlanDatabase {
    let connection = Connection::open_in_memory().unwrap();
    super::super::create_schema(&connection).unwrap();
    PlanDatabase {
        path: Default::default(),
        connection: parking_lot::Mutex::new(connection),
    }
}

fn insert(
    connection: &Connection,
    entity: &str,
    kind: &str,
    state: &str,
    at: Option<u64>,
    error: Option<&str>,
) -> i64 {
    connection
        .execute(
            "INSERT INTO plan_operations(entity_id,kind,state,submitted_at,finished_at,error)
         VALUES (?1,?2,?3,1,?4,?5)",
            params![entity, kind, state, at, error],
        )
        .unwrap();
    connection.last_insert_rowid()
}

fn ids(connection: &Connection) -> Vec<i64> {
    connection
        .prepare("SELECT id FROM plan_operations ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn retention_uses_injected_time_and_preserves_unconfirmed_receipts() {
    let database = database();
    let connection = database.connection.lock();
    let cutoff = NOW - OPERATION_RETENTION_MS;
    insert(
        &connection,
        "old",
        "fixture",
        "committed",
        Some(cutoff - 1),
        None,
    );
    insert(
        &connection,
        "failed",
        "fixture",
        "failed",
        Some(cutoff - 1),
        Some("write refused"),
    );
    let boundary = insert(
        &connection,
        "boundary",
        "fixture",
        "committed",
        Some(cutoff),
        None,
    );
    let pending = insert(&connection, "pending", "persist_plan", "queued", None, None);
    let running = insert(
        &connection,
        "running",
        "persist_plan",
        "running",
        Some(1),
        None,
    );
    let unfinished = insert(
        &connection,
        "unfinished",
        "persist_plan",
        "failed",
        None,
        None,
    );
    let uncertain = insert(
        &connection,
        "uncertain",
        "persist_plan",
        "failed",
        Some(1),
        Some("daemon restarted before commit"),
    );
    assert_eq!(prune(&connection, NOW, 10).unwrap(), 2);
    assert_eq!(
        ids(&connection),
        [boundary, pending, running, unfinished, uncertain]
    );
    assert_eq!(prune(&connection, NOW + 1, 0).unwrap(), 1);
    assert_eq!(ids(&connection), [pending, running, unfinished, uncertain]);
}

#[test]
fn count_bound_keeps_latest_terminal_receipts_with_stable_ties() {
    let database = database();
    let connection = database.connection.lock();
    let newest = insert(
        &connection,
        "newest",
        "fixture",
        "committed",
        Some(NOW),
        None,
    );
    insert(
        &connection,
        "older",
        "fixture",
        "committed",
        Some(NOW - 1),
        None,
    );
    let tie = insert(
        &connection,
        "tie",
        "fixture",
        "failed",
        Some(NOW),
        Some("refused"),
    );
    insert(
        &connection,
        "oldest",
        "fixture",
        "committed",
        Some(NOW - 2),
        None,
    );
    assert_eq!(prune(&connection, NOW, 2).unwrap(), 2);
    assert_eq!(ids(&connection), [newest, tie]);
    assert_eq!(prune(&connection, NOW, 2).unwrap(), 0);
}

#[test]
fn deleting_plan_prunes_its_receipts_and_cascaded_spec_receipts() {
    let database = database();
    {
        let connection = database.connection.lock();
        connection.execute("INSERT INTO plans(id,title,status,project_path,updated_at,header_json) VALUES ('plan','Plan','active','.',1,'{}')", []).unwrap();
        connection.execute("INSERT INTO specs(id,plan_id,project_path,updated_at,row_json) VALUES ('spec','plan','.',1,'{}')", []).unwrap();
        insert(
            &connection,
            "plan",
            "persist_plan",
            "committed",
            Some(NOW),
            None,
        );
        insert(
            &connection,
            "spec",
            "persist_spec",
            "committed",
            Some(NOW),
            None,
        );
        let queued = insert(&connection, "plan", "persist_plan", "queued", None, None);
        let foreign = insert(
            &connection,
            "other",
            "persist_plan",
            "committed",
            Some(NOW),
            None,
        );
        drop(connection);
        database.delete_plan("plan", NOW).unwrap();
        assert_eq!(ids(&database.connection.lock()), [queued, foreign]);
    }
    assert_eq!(database.prune_operations(NOW).unwrap(), 1);
    assert_eq!(database.operation_count_for_test(None), 1);
}

#[test]
fn early_clock_does_not_underflow_retention_cutoff() {
    let database = database();
    let connection = database.connection.lock();
    let id = insert(&connection, "early", "fixture", "committed", Some(0), None);
    assert_eq!(prune(&connection, 0, 10).unwrap(), 0);
    assert_eq!(ids(&connection), [id]);
}
