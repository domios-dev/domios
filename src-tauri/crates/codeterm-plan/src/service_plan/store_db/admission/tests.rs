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
