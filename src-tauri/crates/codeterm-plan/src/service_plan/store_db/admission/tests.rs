use super::*;

#[test]
fn schema_comparison_preserves_column_and_type_token_boundaries() {
    let expected = Connection::open_in_memory().unwrap();
    expected
        .execute_batch("CREATE TABLE probe(id INTEGER)")
        .unwrap();
    let actual = Connection::open_in_memory().unwrap();
    actual
        .execute_batch("CREATE TABLE probe(idINTEGER)")
        .unwrap();
    assert!(validate(&actual, &expected).is_err());
}

#[test]
fn schema_comparison_allows_layout_whitespace_but_preserves_quoted_text() {
    let expected = Connection::open_in_memory().unwrap();
    expected
        .execute_batch("CREATE TABLE probe(id TEXT DEFAULT 'a b')")
        .unwrap();
    let actual = Connection::open_in_memory().unwrap();
    actual
        .execute_batch("CREATE TABLE probe ( id  TEXT DEFAULT 'a b' )")
        .unwrap();
    validate(&actual, &expected).unwrap();
    assert_ne!(normalize("'a b'".into()), normalize("'ab'".into()));
}

fn declared_indexes(connection: &Connection) -> Vec<(String, String)> {
    schema(connection)
        .unwrap()
        .into_iter()
        .filter(|entry| entry.0 == "index")
        .map(|entry| (entry.1, entry.2))
        .collect()
}

#[test]
fn schema_admission_rejects_every_missing_index() {
    let expected = Connection::open_in_memory().unwrap();
    super::super::create_schema(&expected).unwrap();
    let actual = Connection::open_in_memory().unwrap();
    super::super::create_schema(&actual).unwrap();
    let indexes = declared_indexes(&expected);
    assert!(!indexes.is_empty());
    for (name, table) in indexes {
        actual.execute_batch(&format!("DROP INDEX {name}")).unwrap();
        assert!(validate(&actual, &expected).is_err(), "{name} on {table}");
        super::super::create_schema(&actual).unwrap();
    }
}

#[test]
fn schema_admission_rejects_a_wrong_definition_for_a_declared_index_name() {
    let expected = Connection::open_in_memory().unwrap();
    super::super::create_schema(&expected).unwrap();
    for (name, table) in declared_indexes(&expected) {
        let actual = Connection::open_in_memory().unwrap();
        super::super::create_schema(&actual).unwrap();
        let column: String = actual
            .query_row(
                &format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid DESC LIMIT 1"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        actual
            .execute_batch(&format!(
                "DROP INDEX {name}; CREATE INDEX {name} ON {table}(\"{column}\") WHERE 0"
            ))
            .unwrap();
        assert!(validate(&actual, &expected).is_err(), "{name}");
    }
}

#[test]
fn schema_admission_rejects_unowned_extra_indexes() {
    let expected = Connection::open_in_memory().unwrap();
    super::super::create_schema(&expected).unwrap();
    let actual = Connection::open_in_memory().unwrap();
    super::super::create_schema(&actual).unwrap();
    actual
        .execute_batch(
            "DROP INDEX plan_tasks_board_cursor;
             CREATE INDEX unowned_plan_tasks_index ON plan_tasks(plan_id)",
        )
        .unwrap();

    assert!(validate(&actual, &expected).is_err());
}
