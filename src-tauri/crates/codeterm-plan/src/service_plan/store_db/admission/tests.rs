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

#[test]
fn schema_admission_allows_the_missing_managed_board_cursor_index() {
    let expected = Connection::open_in_memory().unwrap();
    super::super::create_schema(&expected).unwrap();
    let actual = Connection::open_in_memory().unwrap();
    super::super::create_schema(&actual).unwrap();
    actual
        .execute_batch("DROP INDEX plan_tasks_board_cursor")
        .unwrap();

    validate(&actual, &expected).unwrap();
}

#[test]
fn schema_admission_rejects_a_wrong_definition_for_the_managed_index_name() {
    let expected = Connection::open_in_memory().unwrap();
    super::super::create_schema(&expected).unwrap();
    let actual = Connection::open_in_memory().unwrap();
    super::super::create_schema(&actual).unwrap();
    actual
        .execute_batch(
            "DROP INDEX plan_tasks_board_cursor;
             CREATE INDEX plan_tasks_board_cursor ON plan_tasks(plan_id)",
        )
        .unwrap();

    assert!(validate(&actual, &expected).is_err());
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
