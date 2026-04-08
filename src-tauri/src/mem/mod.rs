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
pub mod learnings;
pub mod dreaming;
pub mod rerank;
pub mod embeddings;

// TODO(mesh-sync): Cross-machine mem replication anchor.
// Not implemented in 3.x — left intentionally as a hole for the
// scale phase. When we add it, the entry point will live here as a
// `pub mod sync;` with a selective replication policy:
//   - Replicate kinds: rule, preference, skill, fact (curated only)
//   - Never replicate: workspace-scoped items, raw conversations,
//     daily notes, .learnings/ noise, secrets
//   - Modes: manual / scheduled / eventual (per
//     `docs/plans/mem-architecture.md` "Mesh replication policy")
// Until then, mem is per-machine. Each peer keeps its own knowledge.db.

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
    /// Runs migrations on first call. Idempotent. When the `embeddings`
    /// feature is on (default), also initializes the sqlite-vec extension
    /// and the mem_vectors virtual table so semantic recall works out of
    /// the box without a config flip.
    pub fn open(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        std::fs::create_dir_all(&mem_dir)
            .map_err(|e| format!("create mem dir: {}", e))?;
        let blobs_dir = mem_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir)
            .map_err(|e| format!("create blobs dir: {}", e))?;

        // Register sqlite-vec as a global auto-extension BEFORE opening the
        // connection so the vec0 virtual-table module is available. No-op
        // when the embeddings feature is off.
        embeddings::ensure_extension_registered();

        let db_path = mem_dir.join("knowledge.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("open knowledge.db: {}", e))?;

        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;

        // Create the mem_vectors virtual table now that the extension is loaded.
        // Failure here is non-fatal — FTS5 hybrid recall still works.
        if let Err(e) = embeddings::init_extension(&conn) {
            log::warn!("[mem] sqlite-vec init failed: {} — semantic recall disabled", e);
        }

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

    /// Internal: lock-free borrow of the connection mutex for dreaming + tests.
    pub(crate) fn conn_for_dream(&self) -> &parking_lot::Mutex<rusqlite::Connection> {
        &self.conn
    }

    /// Initialize the sqlite-vec extension and mem_vectors table.
    /// No-op when the `embeddings` feature is not compiled in.
    pub fn init_embeddings(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        embeddings::init_extension(&conn)
    }

    /// Hybrid recall: when embeddings feature is on, runs vector ANN over
    /// mem_vectors and merges with FTS5 hybrid scoring. When the feature is
    /// off, falls through to plain `search` (FTS5 hybrid). Same return type.
    pub fn recall(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        #[cfg(feature = "embeddings")]
        {
            let limit = q.limit.unwrap_or(5);
            let conn = self.conn.lock();
            // Vector candidates
            let vec_hits = embeddings::recall(&conn, &q.query, limit * 5).unwrap_or_default();
            if vec_hits.is_empty() {
                return search::run(&conn, q, &self.blobs_dir);
            }
            let mut out = Vec::new();
            for (id, dist) in vec_hits {
                if let Ok(Some(item)) = store::get(&conn, &id, &self.blobs_dir) {
                    out.push(SearchHit { item, score: dist });
                }
            }
            out.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
            out.truncate(limit);
            return Ok(out);
        }
        #[cfg(not(feature = "embeddings"))]
        {
            self.search(q)
        }
    }

    /// Index a mem_item's body in mem_vectors. No-op without embeddings feature.
    pub fn index_embedding(&self, id: &str, body: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        embeddings::index(&conn, id, body)
    }

    /// Save a new mem_item. Returns the assigned slug id.
    /// Runs preference extraction if kind is conversation/note/daily.
    pub fn save(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, false)
    }

    /// Upsert: if a row with the slug-derived id already exists, replace it
    /// in place (preserves id stability). Otherwise behaves like `save`.
    pub fn save_replace(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, true)
    }

    fn save_inner(&self, item: NewMemItem, replace: bool) -> Result<String, String> {
        let conn = self.conn.lock();
        let id = if replace {
            store::upsert(&conn, &item, &self.blobs_dir)?
        } else {
            store::insert(&conn, &item, &self.blobs_dir)?
        };

        // Auto-index in mem_vectors as a background task. The fastembed
        // model is ~50MB and downloads from HuggingFace on first use, so
        // doing this synchronously inside save() would block the caller
        // for tens of seconds on first save. A spawned thread keeps save()
        // fast; the embedding lands eventually. SQLite WAL mode handles
        // the concurrent write safely.
        #[cfg(feature = "embeddings")]
        {
            let id_clone = id.clone();
            let body_clone = item.body.clone();
            let db_path = self.db_path.clone();
            std::thread::spawn(move || {
                embeddings::ensure_extension_registered();
                match rusqlite::Connection::open(&db_path) {
                    Ok(bg_conn) => {
                        match embeddings::index(&bg_conn, &id_clone, &body_clone) {
                            Ok(()) => log::info!("[mem] indexed embedding for {}", id_clone),
                            Err(e) => log::warn!("[mem] bg embedding index failed for {}: {}", id_clone, e),
                        }
                    }
                    Err(e) => log::warn!("[mem] bg conn open failed: {}", e),
                }
            });
        }
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

    /// Partial update of an existing mem_item. Only fields that are Some(_)
    /// are touched; the rest stay as they were. If `body` is updated, the
    /// new content is also re-indexed in mem_vectors (background).
    pub fn update(&self, id: &str, patch: store::MemUpdate) -> Result<bool, String> {
        let body_changed = patch.body.is_some();
        let new_body_for_index = patch.body.clone();
        let updated = {
            let conn = self.conn.lock();
            store::update(&conn, id, &patch, &self.blobs_dir)?
        };
        if updated && body_changed {
            #[cfg(feature = "embeddings")]
            if let Some(body) = new_body_for_index {
                let id_clone = id.to_string();
                let db_path = self.db_path.clone();
                std::thread::spawn(move || {
                    embeddings::ensure_extension_registered();
                    if let Ok(bg_conn) = rusqlite::Connection::open(&db_path) {
                        let _ = embeddings::index(&bg_conn, &id_clone, &body);
                    }
                });
            }
        }
        Ok(updated)
    }

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::run(&conn, q, &self.blobs_dir)
    }

    /// Search with optional LLM rerank pass on the top candidates.
    /// Falls back to plain search if rerank is disabled or unavailable.
    pub fn search_with_rerank(
        &self,
        q: &SearchQuery,
        rerank_cfg: &rerank::RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let mut overfetch = q.clone();
        overfetch.limit = Some(q.limit.unwrap_or(5).max(20));
        let candidates = {
            let conn = self.conn.lock();
            search::run(&conn, &overfetch, &self.blobs_dir)?
        };
        let reranker = rerank::build(rerank_cfg);
        Ok(reranker.rerank(&q.query, candidates, q.limit.unwrap_or(5)))
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

    /// Promote a `kind=learning` row to `kind=fact` (preserves id, body,
    /// relations). Bumps importance to ≥4.0 so dreaming may pull it into L1.
    pub fn promote_learning(&self, id: &str) -> Result<(), String> {
        let item = self.get(id)?.ok_or_else(|| format!("not found: {}", id))?;
        if item.kind != "learning" {
            return Err(format!("can only promote kind=learning, got kind={}", item.kind));
        }
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), id],
        ).map_err(|e| format!("promote: {}", e))?;
        Ok(())
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
    /// Number of rows in mem_vectors. Zero when sqlite-vec extension is
    /// not loaded or background indexing hasn't caught up yet.
    #[serde(default)]
    pub total_vectors: u64,
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
