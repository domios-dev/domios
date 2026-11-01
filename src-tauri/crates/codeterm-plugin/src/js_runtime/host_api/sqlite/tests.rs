use super::pool::POOL_CONNECTIONS_PER_PLUGIN;
use super::*;
use std::sync::atomic::Ordering;

#[test]
fn catalogue_rejection_query_never_materializes_oversized_transcript_rows() {
    let (_dir, db) = fixture_db("CREATE TABLE message(id TEXT PRIMARY KEY, data TEXT, time_updated INTEGER);
        INSERT INTO message VALUES('large', json_object('role', 'user', 'content', printf('%016384d', 0)), 3);
        INSERT INTO message VALUES('current', json_object('role', 'assistant', 'providerID', 'fixture', 'modelID', 'current', 'time', json_object('completed', 1)), 2);
        INSERT INTO message VALUES('old', json_object('role', 'assistant', 'providerID', 'fixture', 'modelID', 'old', 'error', json_object('name', 'APIError', 'data', json_object('statusCode', 410, 'isRetryable', json('false')))), 1);");
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/fixture-provider/src/modelRejections.json"
    )))
    .unwrap();
    let result = rows(
        query_all_json(
            &db,
            fixture["query"].as_str().unwrap(),
            Some("[1024]"),
            4096,
        )
        .unwrap(),
    );
    assert_eq!(result.len(), 2);
    let current: serde_json::Value =
        serde_json::from_str(result[0]["data"].as_str().unwrap()).unwrap();
    let old: serde_json::Value = serde_json::from_str(result[1]["data"].as_str().unwrap()).unwrap();
    assert_eq!(current["modelID"], "current");
    assert_eq!(old["error"]["data"]["isRetryable"], false);
}

#[test]
fn provider_compat_sqlite_gate_covers_raw_and_convenience_host_calls() {
    use crate::js_runtime::{call_json, PluginManifest, PluginVm};
    let (_directory, db) =
        fixture_db("CREATE TABLE fixture(value INTEGER); INSERT INTO fixture VALUES (42);");
    for (detected, enabled, allow) in [
        (Some("1.0.0"), true, true),
        (Some("2.0.0"), true, false),
        (Some("2.0.0-beta.1"), true, false),
        (None, true, false),
        (Some("0.9.0"), true, true),
        (Some("1.20.0"), true, true),
        (Some("bad"), true, false),
        (Some("2.0.0"), false, true),
    ] {
        let json = serde_json::json!({
            "id": "sqlite-gate-fixture", "version": "1.0.0", "displayName": "Fixture",
            "cliCompatibility": {"supported": ["=1.0.0"], "directSqlite": enabled,
                "breaking": [{"range": ">=2.0.0", "reasons": ["sqlite_schema_conversion"]}]}
        })
        .to_string();
        PluginManifest::from_json_str(&json).unwrap();
        let vm = crate::js_runtime::provider_observation::with_fixture_version(detected, || {
            PluginVm::from_source_with("sqlite-gate-fixture", r#"module.exports.default = {
                raw: function(db) { return host.sqliteQueryAllJson(db, 'SELECT value FROM fixture', null); },
                wrapped: function(db) { return host.fs.sqliteQuery(db, 'SELECT value FROM fixture'); },
                awaited: function(db) { return host.fs.sqliteQueryAsync(db, 'SELECT value FROM fixture', null, function(rows) { return rows; }); },
                buildLaunchCommand: function(value) { return value; }
            };"#.into(), Some(json)).unwrap()
        });
        let raw: Option<String> = call_json(&vm, "raw", &[serde_json::json!(db)]).unwrap();
        let wrapped: Vec<serde_json::Value> =
            call_json(&vm, "wrapped", &[serde_json::json!(db)]).unwrap();
        let awaited: Vec<serde_json::Value> =
            call_json(&vm, "awaited", &[serde_json::json!(db)]).unwrap();
        assert_eq!(raw.is_some(), allow);
        assert_eq!(
            awaited, wrapped,
            "the async read keeps the compatibility gate"
        );
        assert_eq!(
            wrapped,
            if allow {
                vec![serde_json::json!({"value": 42})]
            } else {
                Vec::new()
            }
        );
        let command: String = call_json(
            &vm,
            "buildLaunchCommand",
            &[serde_json::json!("opaque-launch")],
        )
        .unwrap();
        assert_eq!(command, "opaque-launch");
    }
}

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

#[test]
fn row_read_error_never_returns_partial_success() {
    let (_directory, db) =
        fixture_db("CREATE TABLE fixture(value INTEGER); INSERT INTO fixture VALUES (1), (2);");
    let result = query_all_json(
        &db,
        "SELECT CASE WHEN value = 2 THEN json_extract('invalid-json', '$') ELSE value END AS value FROM fixture",
        None,
        SQLITE_RESULT_MAX_BYTES,
    ).unwrap();
    assert_eq!(result, None);
}

fn opens(pool: &ConnectionPool) -> usize {
    pool.opens.load(Ordering::Relaxed)
}

fn pool_rows(pool: &ConnectionPool, plugin: &str, db: &str, sql: &str) -> Vec<serde_json::Value> {
    rows(
        pool.query(plugin, db, sql, None, SQLITE_RESULT_MAX_BYTES)
            .unwrap(),
    )
}

#[test]
fn same_path_reuses_one_connection() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES (1);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    for _ in 0..3 {
        assert_eq!(
            pool_rows(&pool, "p", &db, "SELECT v FROM t"),
            vec![serde_json::json!({"v": 1})]
        );
    }
    assert_eq!(opens(&pool), 1);
    assert_eq!(pool.pooled("p"), 1);
    pool_rows(&pool, "other", &db, "SELECT v FROM t");
    assert_eq!(opens(&pool), 2, "pools are per plugin");
}

#[test]
fn pooled_connection_sees_later_commits() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES (1);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    assert_eq!(
        pool_rows(&pool, "p", &db, "SELECT count(*) AS n FROM t"),
        vec![serde_json::json!({"n": 1})]
    );
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("INSERT INTO t VALUES (2);")
        .unwrap();
    assert_eq!(
        pool_rows(&pool, "p", &db, "SELECT count(*) AS n FROM t"),
        vec![serde_json::json!({"n": 2})]
    );
    assert_eq!(opens(&pool), 1);
}

#[test]
fn changed_file_identity_drops_the_pooled_connection() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    pool.state.lock().entries.get_mut("p").unwrap()[0]
        .identity
        .file ^= 1;
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    assert_eq!(opens(&pool), 2);
    assert_eq!(pool.pooled("p"), 1);
}

#[cfg(unix)]
#[test]
fn vanished_or_replaced_file_drops_the_pooled_connection() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES (1);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    std::fs::remove_file(&db).unwrap();
    assert_eq!(
        pool.query("p", &db, "SELECT v FROM t", None, 4096),
        Ok(None)
    );
    assert_eq!(pool.pooled("p"), 0);
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES (2);")
        .unwrap();
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    let replacement = format!("{db}.next");
    rusqlite::Connection::open(&replacement)
        .unwrap()
        .execute_batch("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES (3);")
        .unwrap();
    std::fs::rename(&replacement, &db).unwrap();
    assert_eq!(
        pool_rows(&pool, "p", &db, "SELECT v FROM t"),
        vec![serde_json::json!({"v": 3})]
    );
    assert_eq!(opens(&pool), 3);
}

#[test]
fn pool_is_bounded_per_plugin_with_lru_eviction() {
    let dbs: Vec<_> = (0..POOL_CONNECTIONS_PER_PLUGIN + 2)
        .map(|_| fixture_db("CREATE TABLE t(v INTEGER);"))
        .collect();
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    for (_, db) in &dbs {
        pool_rows(&pool, "p", db, "SELECT v FROM t");
    }
    assert_eq!(pool.pooled("p"), POOL_CONNECTIONS_PER_PLUGIN);
    assert_eq!(opens(&pool), dbs.len());
    pool_rows(&pool, "p", &dbs[dbs.len() - 1].1, "SELECT v FROM t");
    assert_eq!(opens(&pool), dbs.len(), "most recent stays pooled");
    pool_rows(&pool, "p", &dbs[0].1, "SELECT v FROM t");
    assert_eq!(opens(&pool), dbs.len() + 1, "least recent was evicted");
    assert_eq!(pool.pooled("p"), POOL_CONNECTIONS_PER_PLUGIN);
}

#[test]
fn plugin_transaction_is_refused_before_it_can_enter_the_pool() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    assert_eq!(
        pool.query("p", &db, "BEGIN", None, SQLITE_RESULT_MAX_BYTES)
            .unwrap(),
        None
    );
    assert_eq!(pool.pooled("p"), 0);
}

#[test]
fn host_transaction_is_never_returned_to_the_pool() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("host-transaction");
    let mut entry = pool.acquire("host-transaction", &db).unwrap();
    entry.conn = rusqlite::Connection::open(&db).unwrap();
    entry.conn.execute_batch("BEGIN").unwrap();
    assert!(!entry.conn.is_autocommit());
    pool.checkin("host-transaction", entry, false);
    assert_eq!(pool.pooled("host-transaction"), 0);
}

#[test]
fn write_locked_db_waits_the_busy_timeout_then_keeps_the_null_contract() {
    let (_dir, db) = fixture_db("PRAGMA journal_mode=DELETE; CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    assert_eq!(
        pool_rows(&pool, "p", &db, "PRAGMA busy_timeout"),
        vec![serde_json::json!({"timeout": SQLITE_BUSY_TIMEOUT.as_millis() as i64})]
    );
    let writer = rusqlite::Connection::open(&db).unwrap();
    writer
        .execute_batch("BEGIN EXCLUSIVE; INSERT INTO t VALUES (1);")
        .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        pool.query("p", &db, "SELECT v FROM t", None, 4096),
        Ok(None)
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    writer.execute_batch("COMMIT;").unwrap();
    assert_eq!(
        pool_rows(&pool, "p", &db, "SELECT v FROM t"),
        vec![serde_json::json!({"v": 1})]
    );
    assert_eq!(opens(&pool), 1);
}

#[test]
fn pooled_results_match_the_per_call_path() {
    let (_dir, db) = fixture_db(
        "CREATE TABLE t(i INTEGER, r REAL, s TEXT, b BLOB, n TEXT);
         INSERT INTO t VALUES (7, 1.5, 'q\"uote', x'010203', NULL), (8, -2.25, 'x', x'', 'y');",
    );
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    let cases: [(&str, Option<&str>); 5] = [
        ("SELECT * FROM t ORDER BY i", None),
        ("SELECT i, s FROM t WHERE i = ?1", Some("[8]")),
        ("SELECT i, s FROM t WHERE i = ?1", Some("[7]")),
        ("SELECT nope FROM", None),
        ("SELECT ?1", Some("not-json")),
    ];
    for _ in 0..2 {
        for (sql, params) in cases {
            assert_eq!(
                pool.query("p", &db, sql, params, 4096),
                query_all_json(&db, sql, params, 4096),
                "{sql}"
            );
        }
    }
    assert_eq!(opens(&pool), 1);
}

#[test]
fn plugin_reload_drops_its_pooled_connections() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let plugin = "sqlite-pool-reload-fixture";
    let source = r#"module.exports.default = {
        run: function(db) { return host.sqliteQueryAllJson(db, 'SELECT v FROM t', null); }
    };"#;
    let _lease = lease_plugin_sqlite(plugin);
    let vm = crate::js_runtime::vm::PluginVm::from_source(plugin, source.to_string()).unwrap();
    let _: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "run", &[serde_json::json!(db)]).unwrap();
    assert_eq!(POOL.pooled(plugin), 1);
    vm.reload(source.to_string()).unwrap();
    assert_eq!(POOL.pooled(plugin), 0);
    let _: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "run", &[serde_json::json!(db)]).unwrap();
    assert_eq!(POOL.pooled(plugin), 1);
    drop(vm);
    assert_eq!(POOL.pooled(plugin), 0);
}

#[test]
fn lease_keeps_connections_across_calls_and_last_release_closes_them() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let first = pool.lease("p");
    let second = pool.lease("p");
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    assert_eq!((opens(&pool), pool.pooled("p")), (1, 1));
    drop(first);
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    assert_eq!((opens(&pool), pool.pooled("p")), (1, 1));
    drop(second);
    assert_eq!(pool.pooled("p"), 0);
}

#[test]
fn checkin_after_the_last_release_closes_instead_of_pooling() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let lease = pool.lease("p");
    let in_flight = pool.acquire("p", &db).unwrap();
    drop(lease);
    pool.checkin("p", in_flight, false);
    assert_eq!(pool.pooled("p"), 0);
}

#[test]
fn without_a_lease_every_call_opens_and_closes() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES (1);");
    let pool = ConnectionPool::new();
    for _ in 0..3 {
        assert_eq!(
            pool_rows(&pool, "p", &db, "SELECT v FROM t"),
            vec![serde_json::json!({"v": 1})]
        );
        assert_eq!(pool.pooled("p"), 0);
    }
    assert_eq!(opens(&pool), 3);
}

#[test]
fn file_fault_on_a_pooled_connection_evicts_it() {
    let (_dir, db) = fixture_db("PRAGMA journal_mode=DELETE; CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    assert_eq!(pool.pooled("p"), 1);
    {
        use std::io::{Seek, Write};
        let mut file = std::fs::OpenOptions::new().write(true).open(&db).unwrap();
        file.seek(std::io::SeekFrom::Start(0)).unwrap();
        file.write_all(&[0xA5; 100]).unwrap();
    }
    assert_eq!(
        pool.query("p", &db, "SELECT v FROM t", None, 4096),
        Ok(None)
    );
    assert_eq!(pool.pooled("p"), 0);
}

#[test]
fn fault_classification_spares_busy_and_sql_errors() {
    let failure = |code| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code,
                extended_code: 0,
            },
            None,
        )
    };
    assert!(is_connection_fault(&failure(
        rusqlite::ErrorCode::SystemIoFailure
    )));
    assert!(is_connection_fault(&failure(
        rusqlite::ErrorCode::NotADatabase
    )));
    assert!(!is_connection_fault(&failure(
        rusqlite::ErrorCode::DatabaseBusy
    )));
    assert!(!is_connection_fault(&failure(rusqlite::ErrorCode::Unknown)));
}

#[test]
fn concurrent_queries_on_one_path_pool_one_connection_each() {
    let (_dir, db) = fixture_db("CREATE TABLE t(v INTEGER);");
    let pool = ConnectionPool::new();
    let _lease = pool.lease("p");
    let first = pool.acquire("p", &db).unwrap();
    let second = pool.acquire("p", &db).unwrap();
    pool.checkin("p", first, false);
    pool.checkin("p", second, false);
    assert_eq!((opens(&pool), pool.pooled("p")), (2, 2));
    pool_rows(&pool, "p", &db, "SELECT v FROM t");
    assert_eq!((opens(&pool), pool.pooled("p")), (2, 2));
}
