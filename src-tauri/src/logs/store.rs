use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

const SCHEMA: &str = include_str!("schema.sql");
const RING_CAP: usize = 1000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "debug" => Self::Debug,
            "info" => Self::Info,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLog {
    pub id: i64,
    pub ts: i64,
    pub level: String,
    pub source: String,
    pub message: String,
    pub context_json: Option<String>,
}

pub struct LogStore {
    conn: Mutex<Connection>,
    ring: Mutex<VecDeque<PendingLog>>,
    db_path: PathBuf,
    app_handle: Mutex<Option<tauri::AppHandle>>,
}

#[derive(Clone)]
struct PendingLog {
    ts: i64,
    level: String,
    source: String,
    message: String,
    context_json: Option<String>,
}

pub type LogStoreHandle = Arc<LogStore>;

static GLOBAL: OnceLock<LogStoreHandle> = OnceLock::new();

pub fn set_global(handle: LogStoreHandle) {
    let _ = GLOBAL.set(handle);
}

pub fn global() -> Option<LogStoreHandle> {
    GLOBAL.get().cloned()
}

pub fn record(
    level: LogLevel,
    source: &str,
    message: impl Into<String>,
    context: Option<serde_json::Value>,
) {
    let Some(store) = global() else { return };
    store.push(level, source, message.into(), context);
}

impl LogStore {
    pub fn open(data_dir: &Path) -> Result<LogStoreHandle, String> {
        let dir = data_dir.join("logs");
        std::fs::create_dir_all(&dir).map_err(|e| format!("create logs dir: {}", e))?;
        let db_path = dir.join("app_logs.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("open app_logs.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("busy_timeout: {}", e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("init schema: {}", e))?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
            db_path,
            app_handle: Mutex::new(None),
        }))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<LogStoreHandle, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open :memory:: {}", e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("init schema: {}", e))?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
            db_path: PathBuf::from(":memory:"),
            app_handle: Mutex::new(None),
        }))
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn attach_app(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock() = Some(handle);
    }

    pub fn push(
        &self,
        level: LogLevel,
        source: &str,
        message: String,
        context: Option<serde_json::Value>,
    ) {
        let ts = chrono::Utc::now().timestamp_millis();
        let ctx_str = context.as_ref().and_then(|v| serde_json::to_string(v).ok());
        let pending = PendingLog {
            ts,
            level: level.as_str().to_string(),
            source: source.to_string(),
            message,
            context_json: ctx_str,
        };
        let dropped = {
            let mut ring = self.ring.lock();
            let mut dropped = None;
            while ring.len() >= RING_CAP {
                dropped = ring.pop_front();
            }
            ring.push_back(pending.clone());
            dropped
        };
        if let Some(d) = dropped {
            let _ = self.persist(std::iter::once(d));
        }
        if let Some(handle) = self.app_handle.lock().clone() {
            let payload = serde_json::json!({
                "ts": ts,
                "level": pending.level,
                "source": pending.source,
                "message": pending.message,
                "context": context,
            });
            let _ = handle.emit("logs-updated", payload);
        }
    }

    pub fn flush(&self) -> Result<usize, String> {
        let drained: Vec<PendingLog> = self.ring.lock().drain(..).collect();
        let n = drained.len();
        self.persist(drained.into_iter())?;
        Ok(n)
    }

    fn persist<I: IntoIterator<Item = PendingLog>>(&self, rows: I) -> Result<(), String> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| format!("begin tx: {}", e))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO app_logs (ts, level, source, message, context_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| format!("prepare insert: {}", e))?;
            for r in rows {
                stmt.execute(params![r.ts, r.level, r.source, r.message, r.context_json])
                    .map_err(|e| format!("insert log: {}", e))?;
            }
        }
        tx.commit().map_err(|e| format!("commit: {}", e))?;
        Ok(())
    }

    pub fn query(
        &self,
        level: Option<&str>,
        source: Option<&str>,
        since_ms: Option<i64>,
        limit: i64,
    ) -> Result<Vec<AppLog>, String> {
        let _ = self.flush();
        let conn = self.conn.lock();
        let mut where_clauses: Vec<String> = Vec::new();
        if level.is_some() {
            where_clauses.push("level = ?".into());
        }
        if source.is_some() {
            where_clauses.push("source = ?".into());
        }
        if since_ms.is_some() {
            where_clauses.push("ts >= ?".into());
        }
        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };
        let sql = format!(
            "SELECT id, ts, level, source, message, context_json FROM app_logs {} ORDER BY ts DESC, id DESC LIMIT ?",
            where_sql
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare: {}", e))?;
        let mut params_dyn: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(l) = level {
            params_dyn.push(Box::new(l.to_string()));
        }
        if let Some(s) = source {
            params_dyn.push(Box::new(s.to_string()));
        }
        if let Some(s) = since_ms {
            params_dyn.push(Box::new(s));
        }
        params_dyn.push(Box::new(limit));
        let params_iter: Vec<&dyn rusqlite::ToSql> =
            params_dyn.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_iter.iter()), |r| {
                Ok(AppLog {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    level: r.get(2)?,
                    source: r.get(3)?,
                    message: r.get(4)?,
                    context_json: r.get(5)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    pub fn delete_before(&self, before_ms: i64) -> Result<usize, String> {
        let _ = self.flush();
        let conn = self.conn.lock();
        let n = conn
            .execute("DELETE FROM app_logs WHERE ts < ?1", params![before_ms])
            .map_err(|e| format!("delete: {}", e))?;
        Ok(n)
    }

    pub fn start_flush_loop(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let Some(store) = weak.upgrade() else { break };
                if let Err(e) = store.flush() {
                    eprintln!("[logs.flush] {}", e);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_holds_recent_entries() {
        let store = LogStore::open_in_memory().unwrap();
        for i in 0..1500 {
            store.push(LogLevel::Info, "test", format!("msg {}", i), None);
        }
        let n = store.flush().unwrap();
        assert!(n <= RING_CAP, "ring should be at most {} entries", RING_CAP);
        let recent = store.query(None, None, None, 5000).unwrap();
        assert!(recent.len() >= RING_CAP, "persisted >= ring entries");
    }

    #[test]
    fn query_filters_by_level_and_source() {
        let store = LogStore::open_in_memory().unwrap();
        store.push(LogLevel::Info, "a", "first".into(), None);
        store.push(LogLevel::Warn, "a", "second".into(), None);
        store.push(LogLevel::Error, "b", "third".into(), None);
        store.flush().unwrap();
        let warns = store.query(Some("warn"), None, None, 100).unwrap();
        assert_eq!(warns.len(), 1);
        assert_eq!(warns[0].source, "a");
        let from_b = store.query(None, Some("b"), None, 100).unwrap();
        assert_eq!(from_b.len(), 1);
        assert_eq!(from_b[0].level, "error");
    }

    #[test]
    fn delete_before_cleans_old() {
        let store = LogStore::open_in_memory().unwrap();
        store.push(LogLevel::Info, "x", "msg".into(), None);
        store.flush().unwrap();
        let before = chrono::Utc::now().timestamp_millis() + 1000;
        let n = store.delete_before(before).unwrap();
        assert!(n >= 1);
        assert!(store.query(None, None, None, 100).unwrap().is_empty());
    }

    #[test]
    fn context_json_round_trips() {
        let store = LogStore::open_in_memory().unwrap();
        store.push(
            LogLevel::Error,
            "src",
            "msg".into(),
            Some(serde_json::json!({"x": 1, "y": "z"})),
        );
        store.flush().unwrap();
        let rows = store.query(None, None, None, 1).unwrap();
        assert_eq!(rows.len(), 1);
        let ctx = rows[0].context_json.as_deref().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(ctx).unwrap();
        assert_eq!(parsed["x"], 1);
        assert_eq!(parsed["y"], "z");
    }
}
