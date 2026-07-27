use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ROWS: usize = 10_000;
const MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const EVICT_EVERY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivitySource {
    Push,
    Health,
    Peer,
    Cron,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: Option<i64>,
    pub occurred_at: i64,
    pub source: ActivitySource,
    pub source_id: Option<String>,
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_peer: Option<String>,
    pub level: ActivityLevel,
    pub message: String,
    pub meta: serde_json::Value,
}

impl ActivityEvent {
    pub fn push(service_id: &str, pane_id: &str, body_preview: &str) -> Self {
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Push,
            source_id: Some(service_id.into()),
            target: Some(pane_id.into()),
            source_peer: None,
            level: ActivityLevel::Info,
            message: format!(
                "{} → Tab {}: {}",
                service_id,
                pane_id,
                body_preview.chars().take(80).collect::<String>()
            ),
            meta: serde_json::json!({"body_preview": body_preview}),
        }
    }

    pub fn peer(source_id: &str, level_str: &str, message: &str) -> Self {
        let level = match level_str {
            "warn" => ActivityLevel::Warn,
            "error" => ActivityLevel::Error,
            _ => ActivityLevel::Info,
        };
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Peer,
            source_id: Some(source_id.into()),
            target: None,
            source_peer: None,
            level,
            message: message.into(),
            meta: serde_json::json!({}),
        }
    }

    pub fn peer_relay(
        peer_id: &str,
        peer_name: &str,
        target_pane: &str,
        body_preview: &str,
    ) -> Self {
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Push,
            source_id: Some(peer_id.into()),
            target: Some(target_pane.into()),
            source_peer: Some(peer_name.into()),
            level: ActivityLevel::Info,
            message: format!("{} → Tab {}: {}", peer_name, target_pane, body_preview),
            meta: serde_json::json!({"kind": "mesh_relay", "body_preview": body_preview}),
        }
    }

    pub fn health(service_id: &str, transition: (&str, &str)) -> Self {
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Health,
            source_id: Some(service_id.into()),
            target: None,
            source_peer: None,
            level: if transition.1 == "down" {
                ActivityLevel::Error
            } else {
                ActivityLevel::Info
            },
            message: format!("{}: {} → {}", service_id, transition.0, transition.1),
            meta: serde_json::json!({"from": transition.0, "to": transition.1}),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn source_to_str(source: &ActivitySource) -> &'static str {
    match source {
        ActivitySource::Push => "push",
        ActivitySource::Health => "health",
        ActivitySource::Peer => "peer",
        ActivitySource::Cron => "cron",
    }
}

fn level_to_str(level: &ActivityLevel) -> &'static str {
    match level {
        ActivityLevel::Info => "info",
        ActivityLevel::Warn => "warn",
        ActivityLevel::Error => "error",
    }
}

fn parse_source(s: &str) -> Option<ActivitySource> {
    match s {
        "push" => Some(ActivitySource::Push),
        "health" => Some(ActivitySource::Health),
        "peer" => Some(ActivitySource::Peer),
        "cron" => Some(ActivitySource::Cron),
        _ => None,
    }
}

fn parse_level(s: &str) -> ActivityLevel {
    match s {
        "warn" => ActivityLevel::Warn,
        "error" => ActivityLevel::Error,
        _ => ActivityLevel::Info,
    }
}

pub struct ActivityWriter {
    conn: Arc<Mutex<Connection>>,
    insert_count: Arc<Mutex<usize>>,
}

impl ActivityWriter {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            conn,
            insert_count: Arc::new(Mutex::new(0)),
        }
    }

    pub fn record(&self, ev: ActivityEvent) -> rusqlite::Result<i64> {
        let id = {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO activity (occurred_at, source, source_id, target, level, message, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    ev.occurred_at,
                    source_to_str(&ev.source),
                    ev.source_id,
                    ev.target,
                    level_to_str(&ev.level),
                    ev.message,
                    ev.meta.to_string(),
                ],
            )?;
            conn.last_insert_rowid()
        };

        let should_evict = {
            let mut counter = self.insert_count.lock();
            *counter += 1;
            if *counter >= EVICT_EVERY {
                *counter = 0;
                true
            } else {
                false
            }
        };
        if should_evict {
            self.evict_if_needed().ok();
        }
        Ok(id)
    }

    pub fn evict_if_needed(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        let cutoff = now_ms() - MAX_AGE_MS;
        conn.execute("DELETE FROM activity WHERE occurred_at < ?1", [cutoff])?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))?;
        if count > MAX_ROWS as i64 {
            let to_drop = count - MAX_ROWS as i64;
            conn.execute(
                "DELETE FROM activity WHERE id IN (SELECT id FROM activity ORDER BY occurred_at ASC LIMIT ?1)",
                [to_drop],
            )?;
        }
        Ok(())
    }

    pub fn list(
        &self,
        source: Option<ActivitySource>,
        since: Option<i64>,
        limit: Option<usize>,
    ) -> rusqlite::Result<Vec<ActivityEvent>> {
        let conn = self.conn.lock();
        let limit_val = limit.unwrap_or(500) as i64;
        let row_to_event = |row: &rusqlite::Row| -> rusqlite::Result<ActivityEvent> {
            let source_str: String = row.get(2)?;
            let source = parse_source(&source_str).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    format!("unknown source: {}", source_str).into(),
                )
            })?;
            let level_str: String = row.get(5)?;
            let level = parse_level(&level_str);
            let meta_str: Option<String> = row.get(7)?;
            let meta = meta_str
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::json!({}));
            Ok(ActivityEvent {
                id: Some(row.get(0)?),
                occurred_at: row.get(1)?,
                source,
                source_id: row.get(3)?,
                target: row.get(4)?,
                source_peer: None,
                level,
                message: row.get(6)?,
                meta,
            })
        };

        match (source, since) {
            (Some(s), Some(t)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, occurred_at, source, source_id, target, level, message, meta
                     FROM activity WHERE source = ?1 AND occurred_at > ?2
                     ORDER BY occurred_at DESC LIMIT ?3",
                )?;
                let rows =
                    stmt.query_map(params![source_to_str(&s), t, limit_val], row_to_event)?;
                rows.collect()
            }
            (Some(s), None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, occurred_at, source, source_id, target, level, message, meta
                     FROM activity WHERE source = ?1
                     ORDER BY occurred_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![source_to_str(&s), limit_val], row_to_event)?;
                rows.collect()
            }
            (None, Some(t)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, occurred_at, source, source_id, target, level, message, meta
                     FROM activity WHERE occurred_at > ?1
                     ORDER BY occurred_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![t, limit_val], row_to_event)?;
                rows.collect()
            }
            (None, None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, occurred_at, source, source_id, target, level, message, meta
                     FROM activity
                     ORDER BY occurred_at DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map([limit_val], row_to_event)?;
                rows.collect()
            }
        }
    }

    pub fn count(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::store::ServicesStore;

    #[test]
    fn record_push_event_persists_with_source_metadata() {
        let store = ServicesStore::open_in_memory().unwrap();
        let activity = ActivityWriter::new(store.conn());
        activity
            .record(ActivityEvent::push(
                "svc_voice",
                "pane_a3f1",
                "transcript: hello",
            ))
            .unwrap();
        let events = activity.list(None, None, Some(10)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, ActivitySource::Push);
        assert_eq!(events[0].source_id.as_deref(), Some("svc_voice"));
        assert_eq!(events[0].target.as_deref(), Some("pane_a3f1"));
    }

    #[test]
    fn eviction_keeps_recent_drops_old() {
        let store = ServicesStore::open_in_memory().unwrap();
        let activity = ActivityWriter::new(store.conn());
        for i in 0..10003 {
            activity
                .record(ActivityEvent::peer("svc", "info", &format!("e{}", i)))
                .unwrap();
        }
        activity.evict_if_needed().unwrap();
        let count = activity.count().unwrap();
        assert!(count <= 10000, "got {}", count);
    }

    #[test]
    fn list_filters_by_source() {
        let store = ServicesStore::open_in_memory().unwrap();
        let activity = ActivityWriter::new(store.conn());
        activity
            .record(ActivityEvent::push("svc1", "pane1", "x"))
            .unwrap();
        activity
            .record(ActivityEvent::peer("peer1", "info", "msg"))
            .unwrap();
        let pushes = activity
            .list(Some(ActivitySource::Push), None, None)
            .unwrap();
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].source, ActivitySource::Push);
    }
}
