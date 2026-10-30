use super::*;

fn schema(conn: &Connection) -> String {
    conn.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |row| row.get(0)).unwrap()
}

#[test]
fn old_or_unstamped_schema_is_refused_without_changes() {
    for version in [None, Some(0), Some(1), Some(3)] {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE old_items(id TEXT); INSERT INTO old_items VALUES('preserved')").unwrap();
        if let Some(version) = version {
            conn.execute_batch("CREATE TABLE mem_schema_version(version INTEGER NOT NULL)").unwrap();
            conn.execute("INSERT INTO mem_schema_version VALUES(?1)", [version]).unwrap();
        }
        let before = schema(&conn);
        let changes = conn.total_changes();
        let error = run(&conn).unwrap_err().to_string();
        assert!(error.contains("schema"));
        assert_eq!(schema(&conn), before);
        assert_eq!(conn.total_changes(), changes);
        assert_eq!(conn.query_row("SELECT id FROM old_items", [], |row| row.get::<_, String>(0)).unwrap(), "preserved");
    }
}

#[test]
fn current_schema_reopen_keeps_authorship_and_schema_unchanged() {
    let conn = Connection::open_in_memory().unwrap();
    run(&conn).unwrap();
    conn.execute_batch("INSERT INTO mem_items(id,uid,kind,scope,title,created_at,updated_at,author_kind,author_uid,author_friendship,author_label)
        VALUES ('neighbor','neighbor','learning','global','preserved','now','now','neighbor','uid','friendship','Neighbor'),
               ('system','system','fact','global','system','now','now','system',NULL,NULL,'system');").unwrap();
    let before = schema(&conn);
    let changes = conn.total_changes();
    run(&conn).unwrap();
    assert_eq!(schema(&conn), before);
    assert_eq!(conn.total_changes(), changes);
    let author: (String, String, String) = conn.query_row("SELECT author_kind,author_uid,author_friendship FROM mem_items WHERE id='neighbor'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(author, ("neighbor".into(),"uid".into(),"friendship".into()));
    assert!(conn.execute("UPDATE mem_items SET kind='rule' WHERE id='neighbor'", []).is_err());
    assert!(conn.execute("UPDATE mem_items SET kind='fact' WHERE id='neighbor'", []).is_err());
}
