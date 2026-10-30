use super::*;

fn fail(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(
        "CREATE TABLE should_rollback(id INTEGER); INSERT INTO missing_table VALUES(1)",
    )?;
    Ok(())
}

#[test]
fn versioned_step_failure_rolls_back_schema_and_version() {
    let conn = Connection::open_in_memory().unwrap();
    run_steps(&conn, &[baseline]).unwrap();
    let schema: String = conn
        .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(run_steps(&conn, &[baseline, fail]).is_err());
    assert_eq!(
        conn.query_row("SELECT version FROM mem_schema_version", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        schema
    );
    let empty = Connection::open_in_memory().unwrap();
    assert!(run_steps(&empty, &[fail]).is_err());
    assert_eq!(
        empty
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn versioned_author_columns_roll_back_when_a_later_column_fails() {
    let conn = Connection::open_in_memory().unwrap();
    run_steps(&conn, &[baseline]).unwrap();
    conn.execute_batch("ALTER TABLE mem_items ADD COLUMN author_uid TEXT")
        .unwrap();
    let schema: String = conn
        .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(run(&conn).is_err());
    assert_eq!(
        conn.query_row("SELECT version FROM mem_schema_version", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        schema
    );
}
