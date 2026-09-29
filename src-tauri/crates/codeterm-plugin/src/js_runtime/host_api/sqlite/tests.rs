use super::*;

fn fixture_db(setup: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("fixture.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(setup)
        .unwrap();
    let path = db.to_string_lossy().into_owned();
    (dir, path)
}

fn rows(json: Option<String>) -> Vec<serde_json::Value> {
    serde_json::from_str(&json.expect("query result")).unwrap()
}

#[test]
fn every_column_type_round_trips_under_the_cap() {
    let (_dir, db) = fixture_db(
        "CREATE TABLE t(i INTEGER, r REAL, s TEXT, b BLOB, n TEXT);
         INSERT INTO t VALUES (7, 1.5, 'q\"uote', x'010203', NULL);",
    );
    let out = rows(
        query_all_json(
            &db,
            "SELECT i, r, s, b, n FROM t WHERE i = ?1",
            Some("[7]"),
            4096,
        )
        .unwrap(),
    );
    assert_eq!(
        out,
        vec![serde_json::json!({"i": 7, "r": 1.5, "s": "q\"uote", "b": "AQID", "n": null})]
    );
}

#[test]
fn unreadable_db_bad_sql_and_bad_params_keep_the_null_contract() {
    let (_dir, db) = fixture_db("CREATE TABLE t(i INTEGER);");
    assert_eq!(
        query_all_json("/no/such/fixture.db", "SELECT 1", None, 4096),
        Ok(None)
    );
    assert_eq!(
        query_all_json(&db, "SELECT nope FROM", None, 4096),
        Ok(None)
    );
    assert_eq!(
        query_all_json(&db, "SELECT ?1", Some("not-json"), 4096),
        Ok(None)
    );
}

#[test]
fn result_past_the_cap_is_refused_instead_of_collected() {
    let (_dir, db) = fixture_db(
        "CREATE TABLE t(s TEXT);
         WITH RECURSIVE n(k) AS (SELECT 1 UNION ALL SELECT k + 1 FROM n WHERE k < 64)
         INSERT INTO t SELECT printf('%.100c', 'x') FROM n;",
    );
    let under = rows(query_all_json(&db, "SELECT s FROM t LIMIT 10", None, 4096).unwrap());
    assert_eq!(under.len(), 10);
    assert!(query_all_json(&db, "SELECT s FROM t", None, 4096).is_err());
}

#[test]
fn oversized_stored_value_is_never_materialised() {
    let (_dir, db) = fixture_db(
        "CREATE TABLE message(id TEXT, data TEXT);
         INSERT INTO message VALUES ('small', '{\"role\":\"assistant\"}');
         INSERT INTO message VALUES ('huge', '{\"role\":\"user\",\"pad\":\"' || printf('%.20000c', 'x') || '\"}');",
    );
    let cap = 8192;
    assert!(query_all_json(&db, "SELECT data FROM message", None, cap).is_err());
    assert!(
        query_all_json(
            &db,
            "SELECT json_extract(data,'$.role') AS role FROM message",
            None,
            cap
        )
        .is_err(),
        "SQLite must refuse to parse a value past the length limit"
    );
    let guarded = rows(
        query_all_json(
            &db,
            "SELECT id, octet_length(data) AS bytes FROM message \
             WHERE octet_length(data) <= 4096 AND json_extract(data,'$.role') = 'assistant'",
            None,
            cap,
        )
        .unwrap(),
    );
    assert_eq!(
        guarded,
        vec![serde_json::json!({"id": "small", "bytes": 20})]
    );
}

#[test]
fn host_sqlite_bound_breach_throws_range_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE k(id INTEGER);")
        .unwrap();
    let vm = crate::js_runtime::vm::PluginVm::from_source(
        "test",
        r#"
            module.exports.default = {
                run(p, bytes) {
                    try {
                        host.fs.sqliteQuery(p, "SELECT zeroblob(?1) AS z", [bytes]);
                        return "returned";
                    } catch (e) {
                        return e instanceof RangeError ? "range_error" : "other";
                    }
                }
            };
        "#
        .to_string(),
    )
    .expect("vm");
    let over = (SQLITE_RESULT_MAX_BYTES + 1) as u64;
    let r: String = crate::js_runtime::vm::call_json(
        &vm,
        "run",
        &[
            serde_json::json!(db.to_string_lossy()),
            serde_json::json!(over),
        ],
    )
    .unwrap();
    assert_eq!(r, "range_error");
    let r: String = crate::js_runtime::vm::call_json(
        &vm,
        "run",
        &[
            serde_json::json!(db.to_string_lossy()),
            serde_json::json!(3),
        ],
    )
    .unwrap();
    assert_eq!(r, "returned");
}
