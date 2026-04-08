//! Unified memory system: skills, facts, notes, conversations, scripts, peers,
//! preferences, daily, learnings, rules, summaries — one SQLite store, one CLI
//! surface, one query path. See `docs/plans/mem-architecture.md`.
//!
//! All writes flow through `MemManager` so transactions are serialized and
//! plugins / dispatch / REST / Tauri / CLI share one source of truth.

pub mod store;
pub mod search;
pub mod graph;
pub mod layers;
pub mod extract;
pub mod seed;
pub mod ids;

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub use store::{MemItem, MemKind, MemScope, NewMemItem};
pub use graph::{Entity, Relation};
pub use search::{SearchHit, SearchQuery};
pub use layers::WakeUpOptions;

/// Top-level facade. Owns the SQLite connection behind a mutex.
pub struct MemManager {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    blobs_dir: PathBuf,
}

impl MemManager {
    /// Open (or create) the mem database at `data_dir/mem/knowledge.db`.
    /// Runs migrations on first call. Idempotent.
    pub fn open(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        std::fs::create_dir_all(&mem_dir)
            .map_err(|e| format!("create mem dir: {}", e))?;
        let blobs_dir = mem_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir)
            .map_err(|e| format!("create blobs dir: {}", e))?;

        let db_path = mem_dir.join("knowledge.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("open knowledge.db: {}", e))?;

        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;

        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            blobs_dir,
        }))
    }

    /// In-memory instance for tests.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Arc<Self>, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("open :memory:: {}", e))?;
        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;
        let tmp = std::env::temp_dir().join(format!("mem-test-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(tmp.join("blobs")).ok();
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path: tmp.join("knowledge.db"),
            blobs_dir: tmp.join("blobs"),
        }))
    }

    pub fn db_path(&self) -> &std::path::Path { &self.db_path }
    pub fn blobs_dir(&self) -> &std::path::Path { &self.blobs_dir }

    /// Save a new mem_item. Returns the assigned slug id.
    /// Runs preference extraction if kind is conversation/note/daily.
    pub fn save(&self, item: NewMemItem) -> Result<String, String> {
        let conn = self.conn.lock();
        let id = store::insert(&conn, &item, &self.blobs_dir)?;
        // Auto-extract preferences from free-form text
        if matches!(
            item.kind,
            MemKind::Conversation | MemKind::Note | MemKind::Daily
        ) {
            let extracted = extract::extract_preferences(&item.body);
            for prefbody in extracted {
                let child = NewMemItem {
                    kind: MemKind::Preference,
                    scope: item.scope.clone(),
                    title: format!("auto: {}", &prefbody.chars().take(50).collect::<String>()),
                    summary: Some(prefbody.clone()),
                    area: item.area.clone(),
                    tags: vec!["auto-extracted".to_string()],
                    body: prefbody,
                    source: Some(format!("extracted_from:{}", id)),
                    importance: 3.5,
                };
                if let Ok(child_id) = store::insert(&conn, &child, &self.blobs_dir) {
                    let _ = graph::insert_relation(
                        &conn,
                        &Relation {
                            from_id: child_id,
                            relation_type: "extracted_from".into(),
                            to_id: id.clone(),
                            scope: item.scope.as_str().into(),
                            valid_from: chrono::Utc::now().to_rfc3339(),
                            valid_to: None,
                            confidence: 0.8,
                            properties: serde_json::json!({}),
                        },
                    );
                }
            }
        }
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Result<Option<MemItem>, String> {
        let conn = self.conn.lock();
        store::get(&conn, id, &self.blobs_dir)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let conn = self.conn.lock();
        store::delete(&conn, id)
    }

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::run(&conn, q, &self.blobs_dir)
    }

    pub fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String> {
        let conn = self.conn.lock();
        layers::generate(&conn, opts, &self.blobs_dir)
    }

    pub fn graph_create_entity(&self, e: &Entity) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::insert_entity(&conn, e)
    }

    pub fn graph_relate(&self, r: &Relation) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::insert_relation(&conn, r)
    }

    pub fn graph_related(
        &self,
        id: &str,
        rel: Option<&str>,
        as_of: Option<&str>,
    ) -> Result<Vec<Relation>, String> {
        let conn = self.conn.lock();
        graph::related(&conn, id, rel, as_of)
    }

    /// Run bundled starter mem_items seeding once. Idempotent via marker file.
    pub fn seed_bundled_if_needed(&self) -> Result<usize, String> {
        seed::seed_bundled(self)
    }

    /// Stats for diagnostic / wake-up footer.
    pub fn stats(&self) -> Result<MemStats, String> {
        let conn = self.conn.lock();
        store::stats(&conn)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemStats {
    pub total_items: u64,
    pub by_kind: std::collections::HashMap<String, u64>,
    pub total_relations: u64,
    pub total_entities: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_works() {
        let mem = MemManager::open_in_memory().unwrap();
        let stats = mem.stats().unwrap();
        assert_eq!(stats.total_items, 0);
    }

    #[test]
    fn save_and_get_roundtrip() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "test fact".into(),
                summary: Some("a test".into()),
                area: Some("test".into()),
                tags: vec!["tag1".into()],
                body: "the body of the fact".into(),
                source: None,
                importance: 3.0,
            })
            .unwrap();
        assert!(id.starts_with("fact_"));
        let got = mem.get(&id).unwrap().unwrap();
        assert_eq!(got.title, "test fact");
        assert_eq!(got.body, "the body of the fact");
    }

    #[test]
    fn preference_auto_extracted_from_note() {
        let mem = MemManager::open_in_memory().unwrap();
        mem.save(NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "user said".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "I usually prefer PostgreSQL over MySQL for new projects".into(),
            source: None,
            importance: 3.0,
        })
        .unwrap();
        let stats = mem.stats().unwrap();
        // Original note + at least 1 extracted preference
        assert!(stats.total_items >= 2, "expected ≥2 items, got {}", stats.total_items);
        let pref_count = stats.by_kind.get("preference").copied().unwrap_or(0);
        assert!(pref_count >= 1, "expected ≥1 preference, got {}", pref_count);
    }
}
