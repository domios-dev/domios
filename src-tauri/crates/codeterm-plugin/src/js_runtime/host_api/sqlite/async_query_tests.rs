use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::js_runtime::call_gate;
use crate::js_runtime::host_api::PluginSandbox;
use crate::js_runtime::vm::{call_json, PluginVm, VmError};

/// Failure boundary for a wedged expectation, never evidence of success.
const WEDGED: Duration = Duration::from_secs(30);
const GATE_PREFIX: &str = "/*test-gate:";

/// Parks a query job whose SQL opens with `/*test-gate:NAME*/` until the test opens NAME.
pub(super) fn park_on_test_gate(sql: &str) {
    if let Some((name, _)) = sql
        .strip_prefix(GATE_PREFIX)
        .and_then(|rest| rest.split_once("*/"))
    {
        assert!(
            call_gate::wait_opened(name, WEDGED),
            "test gate {name} never opened"
        );
    }
}

const START_REFUSED: &str = "/*test-start-refused*/";

/// A query whose SQL opens with `/*test-start-refused*/` is refused at start, as the job limit does.
pub(super) fn start_refused(sql: &str) -> Option<String> {
    sql.starts_with(START_REFUSED)
        .then(|| serde_json::json!({ "error": "test seam refused the start" }).to_string())
}

static STARTS: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashMap<String, usize>>> =
    std::sync::LazyLock::new(Default::default);

pub(super) fn record_start(plugin_id: &str) {
    *STARTS.lock().entry(plugin_id.to_string()).or_default() += 1;
}

fn starts(plugin_id: &str) -> usize {
    STARTS.lock().get(plugin_id).copied().unwrap_or(0)
}

const FIXTURE: &str = r#"
module.exports.default = {
  sync: function (db, sql, params) { return host.fs.sqliteQuery(db, sql, params); },
  async: function (db, sql, params) {
    return host.fs.sqliteQueryAsync(db, sql, params, function (rows) { return rows; });
  },
  raw: function (db, sql) { return host.sqliteQueryStart(db, sql, null); },
  gated: function (db, sql, entered) {
    var out = host.fs.sqliteQueryAsync(db, sql, [], function (rows) { return rows; });
    host.testGateOpen(entered);
    return out;
  },
  overCap: function (db, bytes) {
    return host.fs.sqliteQueryAsync(db, "SELECT zeroblob(?1) AS z", [bytes], function () { return "returned"; });
  },
  cheap: function () { return 7; },
};
"#;

fn fixture_db() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("fixture.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE t(i INTEGER, r REAL, s TEXT, b BLOB, n);
             INSERT INTO t VALUES (42, 1.5, 'a\"b\\c\u{e9}', x'00ff10', NULL);
             INSERT INTO t VALUES (-5, 0.1, '', x'', 3);",
        )
        .unwrap();
    let path = db.to_string_lossy().into_owned();
    (dir, path)
}

fn vm(plugin_id: &str, sandbox: Option<Arc<PluginSandbox>>) -> Arc<PluginVm> {
    let vm = match sandbox {
        Some(sandbox) => {
            PluginVm::from_source_sandboxed(plugin_id, FIXTURE.to_string(), None, sandbox)
        }
        None => PluginVm::from_source(plugin_id, FIXTURE.to_string()),
    };
    Arc::new(vm.expect("vm"))
}

fn rows(
    vm: &PluginVm,
    method: &str,
    db: &str,
    sql: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    call_json(
        vm,
        method,
        &[serde_json::json!(db), serde_json::json!(sql), params],
    )
    .unwrap()
}

#[test]
fn async_query_returns_the_same_rows_as_the_synchronous_call() {
    let (_dir, db) = fixture_db();
    let vm = vm("sqlite-async-parity", None);
    for (sql, params) in [
        ("SELECT * FROM t ORDER BY i", serde_json::Value::Null),
        ("SELECT i, s FROM t WHERE i = ?1", serde_json::json!([42])),
        (
            "SELECT count(*) AS c, sum(r) AS total FROM t GROUP BY n IS NULL",
            serde_json::Value::Null,
        ),
        ("SELECT * FROM missing_table", serde_json::Value::Null),
    ] {
        let sync = rows(&vm, "sync", &db, sql, params.clone());
        let async_rows = rows(&vm, "async", &db, sql, params);
        assert_eq!(async_rows, sync, "{sql}");
    }
    let all = rows(
        &vm,
        "async",
        &db,
        "SELECT i, b FROM t ORDER BY i DESC",
        serde_json::Value::Null,
    );
    assert_eq!(all[0], serde_json::json!({ "i": 42, "b": "AP8Q" }));
}

#[test]
fn cheap_call_completes_while_a_query_job_is_in_flight() {
    let entered = "sqlite_async_in_flight.entered";
    let release = "sqlite_async_in_flight.release";
    call_gate::reset(entered);
    call_gate::reset(release);
    let (_dir, db) = fixture_db();
    let vm = vm("sqlite-async-in-flight", None);

    let (tx, rx) = mpsc::channel();
    let slow_vm = vm.clone();
    let sql = format!("{GATE_PREFIX}{release}*/ SELECT i FROM t ORDER BY i");
    let slow_db = db.clone();
    std::thread::spawn(move || {
        let out = call_json::<serde_json::Value>(
            &slow_vm,
            "gated",
            &[
                serde_json::json!(slow_db),
                serde_json::json!(sql),
                serde_json::json!(entered),
            ],
        );
        let _ = tx.send(out);
    });
    assert!(
        call_gate::wait_opened(entered, WEDGED),
        "query job never started"
    );

    let (cheap_tx, cheap_rx) = mpsc::channel();
    let cheap_vm = vm.clone();
    std::thread::spawn(move || {
        let _ = cheap_tx.send(call_json::<i64>(&cheap_vm, "cheap", &[]));
    });
    let cheap = cheap_rx
        .recv_timeout(WEDGED)
        .expect("a cheap call waited behind the in-flight query: the VM is held");
    assert_eq!(cheap.unwrap(), 7);
    assert!(!call_gate::is_open(release), "the query was still parked");

    call_gate::open(release);
    let slow = rx
        .recv_timeout(WEDGED)
        .expect("query never resumed")
        .unwrap();
    assert_eq!(slow, serde_json::json!([{ "i": -5 }, { "i": 42 }]));
    call_gate::reset(entered);
    call_gate::reset(release);
}

#[test]
fn async_query_keeps_the_sandbox_read_gate() {
    let (dir, db) = fixture_db();
    let other = tempfile::tempdir().unwrap();
    let confined = vm(
        "sqlite-async-confined",
        Some(Arc::new(
            PluginSandbox::restricted(vec![other.path().to_path_buf()], vec![])
                .with_identity_and_perms("sqlite-async-confined", &Default::default()),
        )),
    );
    let sql = "SELECT i FROM t";
    assert_eq!(
        rows(&confined, "async", &db, sql, serde_json::Value::Null),
        serde_json::json!([])
    );
    let raw: String = call_json(
        &confined,
        "raw",
        &[serde_json::json!(db), serde_json::json!(sql)],
    )
    .unwrap();
    assert_eq!(raw, r#"{"rows":null}"#, "a denied db never starts a job");

    let granted = vm(
        "sqlite-async-granted",
        Some(Arc::new(
            PluginSandbox::restricted(vec![dir.path().to_path_buf()], vec![])
                .with_identity_and_perms("sqlite-async-granted", &Default::default()),
        )),
    );
    assert_eq!(
        rows(&granted, "async", &db, sql, serde_json::Value::Null)
            .as_array()
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn async_query_cap_breach_throws_in_the_continuation() {
    let (_dir, db) = fixture_db();
    let vm = vm("sqlite-async-cap", None);
    let over = (super::SQLITE_RESULT_MAX_BYTES + 1) as u64;
    let error = call_json::<String>(
        &vm,
        "overCap",
        &[serde_json::json!(db), serde_json::json!(over)],
    )
    .unwrap_err();
    assert!(
        matches!(&error, VmError::Script(msg) if msg.contains("sqliteQuery")),
        "{error:?}"
    );
    let small: String = call_json(
        &vm,
        "overCap",
        &[serde_json::json!(db), serde_json::json!(3)],
    )
    .unwrap();
    assert_eq!(small, "returned");
}

#[test]
fn timed_out_query_keeps_running_and_the_next_call_takes_its_rows() {
    let plugin = "sqlite-async-retained";
    let release = "sqlite_async_retained.release";
    call_gate::reset(release);
    let (_dir, db) = fixture_db();
    let vm = vm(plugin, None);
    let sql = format!("{GATE_PREFIX}{release}*/ SELECT i FROM t ORDER BY i");
    let call = |vm: &PluginVm| {
        call_json::<serde_json::Value>(
            vm,
            "async",
            &[
                serde_json::json!(db),
                serde_json::json!(sql),
                serde_json::Value::Null,
            ],
        )
    };

    crate::js_runtime::host_exec::set_test_wait_bound(plugin, Duration::from_millis(50));
    assert!(matches!(call(&vm), Err(VmError::AsyncJobTimedOut)));
    assert_eq!(starts(plugin), 1);
    assert_eq!(
        crate::js_runtime::host_exec::plugin_job_count(plugin),
        1,
        "the timed-out query is still running"
    );

    crate::js_runtime::host_exec::set_test_wait_bound(plugin, WEDGED);
    call_gate::open(release);
    assert_eq!(
        call(&vm).unwrap(),
        serde_json::json!([{ "i": -5 }, { "i": 42 }])
    );
    assert_eq!(starts(plugin), 1, "the second call awaited the same job");
    assert_eq!(
        crate::js_runtime::host_exec::plugin_job_count(plugin),
        0,
        "the consuming poll removed the job"
    );

    assert_eq!(call(&vm).unwrap().as_array().map(Vec::len), Some(2));
    assert_eq!(starts(plugin), 2, "a consumed key starts a fresh job");
    call_gate::reset(release);
}

#[test]
fn refused_start_falls_back_to_the_synchronous_rows() {
    let (_dir, db) = fixture_db();
    let vm = vm("sqlite-async-refused", None);
    let sql = format!("{START_REFUSED} SELECT * FROM t ORDER BY i");
    let sync = rows(&vm, "sync", &db, &sql, serde_json::Value::Null);
    assert_eq!(sync.as_array().map(Vec::len), Some(2));
    assert_eq!(rows(&vm, "async", &db, &sql, serde_json::Value::Null), sync);
    assert_eq!(starts("sqlite-async-refused"), 0);
}
