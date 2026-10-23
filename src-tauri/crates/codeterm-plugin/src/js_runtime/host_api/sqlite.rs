//! `host.sqliteQueryAllJson`: a read-only query streamed into one JSON array
//! whose size, and every value SQLite materialises for it, stay under a cap.

use std::collections::HashMap;
use std::io::Write;
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;

mod pool;

use pool::ConnectionPool;
pub use pool::PluginSqliteLease;

/// Upper bound for one query's JSON result and for any single SQLite value.
const SQLITE_RESULT_MAX_BYTES: usize = 64 << 20;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn host_query(
    ctx: &rquickjs::Ctx<'_>,
    sandbox: &super::PluginSandbox,
    observation: &super::super::provider_observation::ProviderObservation,
    db: &str,
    sql: &str,
    params_json: Option<&str>,
) -> rquickjs::Result<Option<String>> {
    if !query_allowed(sandbox, observation, db) {
        return Ok(None);
    }
    POOL.query(
        sandbox.plugin_id(),
        db,
        sql,
        params_json,
        SQLITE_RESULT_MAX_BYTES,
    )
    .map_err(|error| {
        log::warn!("[plugin] {error} (db {db})");
        rquickjs::Exception::throw_range(ctx, &error)
    })
}

/// The same gate as `host_query`, applied before the job starts; the query
/// then runs on a host thread while the VM is free for other calls.
pub(super) fn host_query_start(
    sandbox: &super::PluginSandbox,
    observation: &super::super::provider_observation::ProviderObservation,
    db: String,
    sql: String,
    params_json: Option<String>,
) -> String {
    if !query_allowed(sandbox, observation, &db) {
        return QUERY_JOB_NO_ROWS.to_string();
    }
    #[cfg(test)]
    if let Some(refused) = async_query_tests::start_refused(&sql) {
        return refused;
    }
    let plugin_id = sandbox.plugin_id();
    let key = (db, sql, params_json);
    let mut in_flight = IN_FLIGHT.lock();
    let jobs = in_flight.entry(plugin_id.to_string()).or_default();
    jobs.retain(|_, job| super::super::host_exec::retained_job_live(plugin_id, job));
    if let Some(job) = jobs.get(&key) {
        if super::super::host_exec::attach_retained(plugin_id, job) {
            return serde_json::json!({ "jobId": job }).to_string();
        }
        jobs.remove(&key);
    }
    if jobs.len() >= MAX_IN_FLIGHT_QUERIES_PER_PLUGIN {
        return serde_json::json!({
            "error": format!("plugin {plugin_id} reached the in-flight sqlite query limit ({MAX_IN_FLIGHT_QUERIES_PER_PLUGIN})")
        })
        .to_string();
    }
    let (db, sql, params_json) = key.clone();
    let worker_plugin = plugin_id.to_string();
    let started = super::super::host_exec::start_retained_json_job(plugin_id, move || {
        #[cfg(test)]
        async_query_tests::park_on_test_gate(&sql);
        let result = POOL.query(
            &worker_plugin,
            &db,
            &sql,
            params_json.as_deref(),
            SQLITE_RESULT_MAX_BYTES,
        );
        query_job_result(&db, result)
    });
    match started {
        Ok(job_id) => {
            #[cfg(test)]
            async_query_tests::record_start(plugin_id);
            jobs.insert(key, job_id.clone());
            serde_json::json!({ "jobId": job_id }).to_string()
        }
        Err(error) => serde_json::json!({ "error": error }).to_string(),
    }
}

/// One job per distinct (db, sql, params): a call arriving while that query is
/// still running, or after its waiter timed out, awaits the same job.
const MAX_IN_FLIGHT_QUERIES_PER_PLUGIN: usize = 32;
type QueryKey = (String, String, Option<String>);
/// Taken before `host_exec`'s job map, never while holding it.
static IN_FLIGHT: LazyLock<Mutex<HashMap<String, HashMap<QueryKey, String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const QUERY_JOB_NO_ROWS: &str = "{\"rows\":null}";

fn query_job_result(db: &str, result: Result<Option<String>, String>) -> String {
    match result {
        Ok(Some(rows)) => format!("{{\"rows\":{rows}}}"),
        Ok(None) => QUERY_JOB_NO_ROWS.to_string(),
        Err(error) => {
            log::warn!("[plugin] {error} (db {db})");
            serde_json::json!({ "rangeError": error }).to_string()
        }
    }
}

fn query_allowed(
    sandbox: &super::PluginSandbox,
    observation: &super::super::provider_observation::ProviderObservation,
    db: &str,
) -> bool {
    if !sandbox.can_read(db) {
        log::warn!("[plugin] denied sqliteQuery outside sandbox: {db}");
        return false;
    }
    if let Some(compatibility) = observation.sqlite_compatibility(sandbox) {
        if !compatibility.allows_sqlite_read() {
            log::debug!(
                "[provider-compatibility] direct_sqlite disabled verdict={:?} reasons={:?}",
                compatibility.verdict,
                compatibility.reasons
            );
            return false;
        }
    }
    true
}

static POOL: LazyLock<ConnectionPool> = LazyLock::new(ConnectionPool::new);

/// Pools `plugin_id`'s sqlite connections for the guard's lifetime; take it for
/// a long-lived reader such as a structured-chat poller, never per call.
pub fn lease_plugin_sqlite(plugin_id: &str) -> PluginSqliteLease<'static> {
    POOL.lease(plugin_id)
}

pub(in crate::js_runtime) fn drop_plugin_connections(plugin_id: &str) {
    POOL.drop_plugin(plugin_id);
}

impl ConnectionPool {
    /// `Ok(None)` keeps the null contract for an unreadable db, bad SQL, or bad
    /// params; `Err` is a cap breach the caller surfaces as a JS exception.
    fn query(
        &self,
        plugin_id: &str,
        db_path: &str,
        sql: &str,
        params_json: Option<&str>,
        max_bytes: usize,
    ) -> Result<Option<String>, String> {
        let access = crate::bundle_access::BundleAccess::for_plugin(plugin_id);
        let _reader = access.read_recursive();
        let Some(params) = parse_params(params_json) else {
            return Ok(None);
        };
        let Some(entry) = self.acquire(plugin_id, db_path) else {
            return Ok(None);
        };
        let mut faulted = false;
        let result = query_connection(&entry.conn, db_path, sql, params, max_bytes, &mut faulted);
        self.checkin(plugin_id, entry, faulted);
        result
    }
}

fn parse_params(params_json: Option<&str>) -> Option<Vec<serde_json::Value>> {
    match params_json {
        Some(json) => serde_json::from_str(json).ok(),
        None => Some(Vec::new()),
    }
}

fn open_read_only(db_path: &str) -> Option<rusqlite::Connection> {
    let conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(error) => {
            log::debug!("[sqlite-session-probe] stage=open db={db_path} error={error}");
            return None;
        }
    };
    if let Err(error) = conn.busy_timeout(SQLITE_BUSY_TIMEOUT) {
        log::debug!("[sqlite-session-probe] stage=busy_timeout db={db_path} error={error}");
        return None;
    }
    Some(conn)
}

/// The unpooled per-call path, kept as the parity reference for the pool.
#[cfg(test)]
fn query_all_json(
    db_path: &str,
    sql: &str,
    params_json: Option<&str>,
    max_bytes: usize,
) -> Result<Option<String>, String> {
    let Some(params) = parse_params(params_json) else {
        return Ok(None);
    };
    let Some(conn) = open_read_only(db_path) else {
        return Ok(None);
    };
    query_connection(&conn, db_path, sql, params, max_bytes, &mut false)
}

fn query_connection(
    conn: &rusqlite::Connection,
    db_path: &str,
    sql: &str,
    params: Vec<serde_json::Value>,
    max_bytes: usize,
    faulted: &mut bool,
) -> Result<Option<String>, String> {
    // SQLite refuses to load a value past this length instead of allocating it.
    conn.set_limit(
        Limit::SQLITE_LIMIT_LENGTH,
        i32::try_from(max_bytes).unwrap_or(i32::MAX),
    );
    let mut stmt = match conn.prepare_cached(sql) {
        Ok(stmt) => stmt,
        Err(error) => {
            log::debug!("[sqlite-session-probe] stage=prepare db={db_path} error={error}");
            *faulted = is_connection_fault(&error);
            return Ok(None);
        }
    };
    let col_names: Vec<String> = (0..stmt.column_count())
        .map(|i| {
            stmt.column_name(i)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| format!("col_{i}"))
        })
        .collect();
    let bound: Vec<Box<dyn rusqlite::ToSql>> = params.into_iter().map(bind_param).collect();
    let bound_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
    let mut rows = match stmt.query(rusqlite::params_from_iter(bound_refs)) {
        Ok(rows) => rows,
        Err(error) => {
            log::debug!("[sqlite-session-probe] stage=query db={db_path} error={error}");
            *faulted = is_connection_fault(&error);
            return Ok(None);
        }
    };

    let mut out: Vec<u8> = vec![b'['];
    loop {
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(error) if is_too_big(&error) => return Err(value_too_big(max_bytes)),
            Err(error) => {
                log::debug!("[sqlite-session-probe] stage=row db={db_path} error={error}");
                *faulted = is_connection_fault(&error);
                return Ok(None);
            }
        };
        if out.len() > 1 {
            out.push(b',');
        }
        out.push(b'{');
        for (i, name) in col_names.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            write_json_str(&mut out, name);
            out.push(b':');
            match row.get_ref(i) {
                Ok(ValueRef::Integer(v)) => {
                    let _ = write!(out, "{v}");
                }
                Ok(ValueRef::Real(v)) => {
                    let _ = serde_json::to_writer(&mut out, &v);
                }
                Ok(ValueRef::Text(bytes)) => {
                    ensure_room(&out, bytes.len(), max_bytes)?;
                    write_json_str(&mut out, &String::from_utf8_lossy(bytes));
                }
                Ok(ValueRef::Blob(bytes)) => {
                    ensure_room(&out, bytes.len().div_ceil(3) * 4, max_bytes)?;
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    write_json_str(&mut out, &encoded);
                }
                Ok(ValueRef::Null) | Err(_) => out.extend_from_slice(b"null"),
            }
            ensure_room(&out, 0, max_bytes)?;
        }
        out.push(b'}');
    }
    out.push(b']');
    ensure_room(&out, 0, max_bytes)?;
    Ok(String::from_utf8(out).ok())
}

fn bind_param(v: serde_json::Value) -> Box<dyn rusqlite::ToSql> {
    match v {
        serde_json::Value::Null => Box::new(rusqlite::types::Null),
        serde_json::Value::Bool(b) => Box::new(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        serde_json::Value::String(s) => Box::new(s),
        other => Box::new(other.to_string()),
    }
}

fn write_json_str(out: &mut Vec<u8>, s: &str) {
    let _ = serde_json::to_writer(out, s);
}

fn ensure_room(out: &[u8], incoming: usize, max_bytes: usize) -> Result<(), String> {
    if out.len().saturating_add(incoming) > max_bytes {
        return Err(format!(
            "sqliteQuery result exceeds {max_bytes} bytes; select narrower columns or add a LIMIT"
        ));
    }
    Ok(())
}

/// Errors that mean the handle no longer reads the file the path names.
fn is_connection_fault(error: &rusqlite::Error) -> bool {
    use rusqlite::ErrorCode::*;
    matches!(
        error,
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(e.code, SystemIoFailure | CannotOpen | DatabaseCorrupt | NotADatabase | FileLockingProtocolFailed)
    )
}

fn is_too_big(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::TooBig
    )
}

fn value_too_big(max_bytes: usize) -> String {
    format!(
        "sqliteQuery touched a value over {max_bytes} bytes; filter large columns with octet_length() before reading them"
    )
}

#[cfg(test)]
mod async_query_tests;
#[cfg(test)]
mod tests;
