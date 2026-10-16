//! `host.sqliteQueryAllJson`: a read-only query streamed into one JSON array
//! whose size, and every value SQLite materialises for it, stay under a cap.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;

/// Upper bound for one query's JSON result and for any single SQLite value.
const SQLITE_RESULT_MAX_BYTES: usize = 64 << 20;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const POOL_CONNECTIONS_PER_PLUGIN: usize = 8;

pub(super) fn host_query(
    ctx: &rquickjs::Ctx<'_>,
    sandbox: &super::PluginSandbox,
    observation: &super::super::provider_observation::ProviderObservation,
    db: &str,
    sql: &str,
    params_json: Option<&str>,
) -> rquickjs::Result<Option<String>> {
    if !sandbox.can_read(db) {
        log::warn!("[plugin] denied sqliteQuery outside sandbox: {db}");
        return Ok(None);
    }
    if let Some(compatibility) = observation.sqlite_compatibility(sandbox) {
        if !compatibility.allows_sqlite_read() {
            log::debug!(
                "[provider-compatibility] direct_sqlite disabled verdict={:?} reasons={:?}",
                compatibility.verdict,
                compatibility.reasons
            );
            return Ok(None);
        }
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

static POOL: LazyLock<ConnectionPool> = LazyLock::new(ConnectionPool::new);

pub(in crate::js_runtime) fn drop_plugin_connections(plugin_id: &str) {
    POOL.drop_plugin(plugin_id);
}

/// Read-only connections reused per (plugin id, canonical db path), LRU-bounded.
/// `entries` is a leaf lock held only for map edits: never across a query, a VM
/// call or a mux guard, and connections open and close outside it.
struct ConnectionPool {
    entries: Mutex<HashMap<String, Vec<PooledConnection>>>,
    opens: AtomicUsize,
}

struct PooledConnection {
    requested: String,
    canonical: PathBuf,
    identity: FileIdentity,
    conn: rusqlite::Connection,
}

/// Which file a path names, so a replaced db never serves the old inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume: u64,
    file: u128,
}

impl FileIdentity {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(Self {
                volume: metadata.dev(),
                file: u128::from(metadata.ino()),
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            Some(Self {
                volume: 0,
                file: u128::from(metadata.creation_time()),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = metadata;
            Some(Self { volume: 0, file: 0 })
        }
    }
}

impl ConnectionPool {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            opens: AtomicUsize::new(0),
        }
    }

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
        let Some(params) = parse_params(params_json) else {
            return Ok(None);
        };
        let resolved = std::fs::canonicalize(db_path)
            .ok()
            .and_then(|canonical| Some((FileIdentity::of(&canonical)?, canonical)));
        let Some((identity, canonical)) = resolved else {
            drop(self.evict_requested(plugin_id, db_path));
            log::debug!("[sqlite-session-probe] stage=open db={db_path} error=missing");
            return Ok(None);
        };
        let conn = match self.checkout(plugin_id, &canonical, identity) {
            Some(conn) => conn,
            None => {
                let Some(conn) = open_read_only(db_path) else {
                    return Ok(None);
                };
                self.opens.fetch_add(1, Ordering::Relaxed);
                conn
            }
        };
        let result = query_connection(&conn, db_path, sql, params, max_bytes);
        // A plugin-issued BEGIN would pin a stale snapshot for every later call.
        if conn.is_autocommit() {
            self.checkin(
                plugin_id,
                PooledConnection {
                    requested: db_path.to_string(),
                    canonical,
                    identity,
                    conn,
                },
            );
        }
        result
    }

    fn checkout(
        &self,
        plugin_id: &str,
        canonical: &Path,
        identity: FileIdentity,
    ) -> Option<rusqlite::Connection> {
        let taken = {
            let mut entries = self.entries.lock();
            let list = entries.get_mut(plugin_id)?;
            let index = list.iter().position(|entry| entry.canonical == canonical)?;
            list.remove(index)
        };
        (taken.identity == identity).then_some(taken.conn)
    }

    fn checkin(&self, plugin_id: &str, entry: PooledConnection) {
        let evicted = {
            let mut entries = self.entries.lock();
            let list = entries.entry(plugin_id.to_string()).or_default();
            let mut evicted = Vec::new();
            if let Some(index) = list.iter().position(|e| e.canonical == entry.canonical) {
                evicted.push(list.remove(index));
            }
            list.push(entry);
            while list.len() > POOL_CONNECTIONS_PER_PLUGIN {
                evicted.push(list.remove(0));
            }
            evicted
        };
        drop(evicted);
    }

    fn evict_requested(&self, plugin_id: &str, requested: &str) -> Vec<PooledConnection> {
        let mut entries = self.entries.lock();
        let Some(list) = entries.get_mut(plugin_id) else {
            return Vec::new();
        };
        let (gone, kept) = std::mem::take(list)
            .into_iter()
            .partition(|entry| entry.requested == requested);
        *list = kept;
        gone
    }

    fn drop_plugin(&self, plugin_id: &str) {
        let removed = self.entries.lock().remove(plugin_id);
        drop(removed);
    }

    #[cfg(test)]
    fn pooled(&self, plugin_id: &str) -> usize {
        self.entries.lock().get(plugin_id).map_or(0, Vec::len)
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
    query_connection(&conn, db_path, sql, params, max_bytes)
}

fn query_connection(
    conn: &rusqlite::Connection,
    db_path: &str,
    sql: &str,
    params: Vec<serde_json::Value>,
    max_bytes: usize,
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
mod tests;
