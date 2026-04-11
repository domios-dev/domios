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
pub mod activity;
pub mod workers;
pub mod auto_extract;

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
        // Allow concurrent writers to wait up to 5s instead of failing immediately
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;

        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;

        // Create the mem_vectors virtual table now that the extension is loaded.
        // Failure here is non-fatal — FTS5 hybrid recall still works.
        if let Err(e) = embeddings::init_extension(&conn) {
            log::warn!("[mem] sqlite-vec init failed: {} — semantic recall disabled", e);
        }

        // One-shot repair pass on startup. Fixes: orphan entities (no refs),
        // bogus category/tech nodes, invalid temporal windows, empty titles.
        if let Err(e) = store::repair_inconsistencies(&conn) {
            log::warn!("[mem] repair pass: {}", e);
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

}

/// Query mem_items without corresponding vectors (free function for use in threads).
#[cfg(feature = "embeddings")]
fn query_missing_vectors(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let sql = r#"SELECT m.id, COALESCE(m.body_inline, '')
                 FROM mem_items m
                 LEFT JOIN mem_vectors v ON v.mem_id = m.id
                 WHERE v.mem_id IS NULL AND m.body_inline IS NOT NULL AND LENGTH(m.body_inline) > 10"#;
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}

impl MemManager {
    /// Re-index items that have no vector in mem_vectors. Runs in a background
    /// thread. On first call (startup), indexes all missing items then starts
    /// a periodic loop that checks every 30s for new unindexed items.
    /// Catches: failed bg indexing, model download timeouts, items created by
    /// workers or other processes. Idempotent — skips already indexed.
    #[cfg(feature = "embeddings")]
    pub fn reindex_missing(self: &Arc<Self>) {
        let db_path = self.db_path.clone();
        let blobs_dir = self.blobs_dir.clone();
        std::thread::spawn(move || {
            embeddings::ensure_extension_registered();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.busy_timeout(std::time::Duration::from_secs(5));
                    c
                }
                Err(e) => { log::warn!("[mem] reindex: open failed: {}", e); return; }
            };
            // Find mem_items that have no row in mem_vectors
            let mut missing: Vec<(String, String)> = query_missing_vectors(&conn);
            // Initial pass + periodic loop
            loop {
                if missing.is_empty() {
                    log::debug!("[mem] reindex: all items indexed");
                } else {
                    log::info!("[mem] reindex: {} items missing vectors, indexing...", missing.len());
                    let mut indexed = 0;
                    for (id, body) in &missing {
                        let actual_body = if body.is_empty() {
                            let blob_path: Option<String> = conn.query_row(
                                "SELECT body_blob_path FROM mem_items WHERE id = ?1",
                                rusqlite::params![id],
                                |r| r.get(0),
                            ).ok().flatten();
                            if let Some(rel) = blob_path {
                                std::fs::read_to_string(blobs_dir.join(rel)).unwrap_or_default()
                            } else {
                                continue;
                            }
                        } else {
                            body.clone()
                        };
                        match embeddings::index(&conn, id, &actual_body) {
                            Ok(()) => { indexed += 1; }
                            Err(e) => { log::warn!("[mem] reindex {}: {}", id, e); }
                        }
                    }
                    if indexed > 0 {
                        log::info!("[mem] reindex: indexed {} items", indexed);
                    }
                }

                // Sleep 30s, then re-check for new unindexed items
                std::thread::sleep(std::time::Duration::from_secs(30));

                // Re-query missing items for next iteration
                missing = query_missing_vectors(&conn);
            }
        });
    }

    #[cfg(not(feature = "embeddings"))]
    pub fn reindex_missing(self: &Arc<Self>) {}

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

    /// Hybrid recall: RRF fusion of FTS5 + vector ANN results.
    /// Always runs both backends and merges via Reciprocal Rank Fusion
    /// for maximum recall quality (typo-tolerant + keyword-precise).
    /// Falls back to FTS5-only when embeddings feature is off.
    pub fn recall(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::hybrid_rrf(&conn, q, &self.blobs_dir)
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

        // Dedup check: skip if an item with same kind+title already exists
        // (unless replace mode, which is intentional overwrite).
        if !replace && !item.title.is_empty() {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM mem_items WHERE kind = ?1 AND title = ?2 LIMIT 1",
                    rusqlite::params![item.kind.as_str(), &item.title],
                    |r| r.get(0),
                )
                .ok();
            if let Some(dup_id) = existing {
                log::info!("[mem] dedup: skipping save, existing item {} has same kind+title", dup_id);
                return Ok(dup_id);
            }
        }

        let id = if replace {
            store::upsert(&conn, &item, &self.blobs_dir)?
        } else {
            store::insert(&conn, &item, &self.blobs_dir)?
        };

        // Auto-connect: every item gets a `kind_of` relation to its kind group entity.
        // This prevents orphan nodes in the graph — every node has at least one edge.
        {
            let kind_entity_id = format!("kind_{}", item.kind.as_str());
            let _ = graph::insert_entity(&conn, &Entity {
                id: kind_entity_id.clone(),
                entity_type: "Kind".into(),
                scope: "global".into(),
                properties: serde_json::json!({"label": item.kind.as_str()}),
            });
            let _ = graph::insert_relation(&conn, &Relation {
                from_id: id.clone(),
                relation_type: "kind_of".into(),
                to_id: kind_entity_id,
                scope: "global".into(),
                valid_from: chrono::Utc::now().to_rfc3339(),
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
            });
            // Also connect to area group if specified
            if let Some(ref area) = item.area {
                let area_entity_id = format!("area_{}", area);
                let _ = graph::insert_entity(&conn, &Entity {
                    id: area_entity_id.clone(),
                    entity_type: "Area".into(),
                    scope: "global".into(),
                    properties: serde_json::json!({"label": area}),
                });
                let _ = graph::insert_relation(&conn, &Relation {
                    from_id: id.clone(),
                    relation_type: "belongs_to".into(),
                    to_id: area_entity_id,
                    scope: "global".into(),
                    valid_from: chrono::Utc::now().to_rfc3339(),
                    valid_to: None,
                    confidence: 1.0,
                    properties: serde_json::json!({}),
                });
            }
        }

        // Auto-index in mem_vectors as a background task. The fastembed
        // model is ~50MB and downloads from HuggingFace on first use, so
        // doing this synchronously inside save() would block the caller
        // for tens of seconds on first save. A spawned thread keeps save()
        // fast; the embedding lands eventually. SQLite WAL mode handles
        // the concurrent write safely.
        #[cfg(feature = "embeddings")]
        {
            let id_clone = id.clone();
            // For file-backed items, read full content from disk for vectorization.
            // item.body may be just a short summary.
            let embed_text = if let Some(ref src) = item.source {
                if let Some(path) = src.strip_prefix("file:") {
                    std::fs::read_to_string(path).unwrap_or_else(|_| item.body.clone())
                } else {
                    item.body.clone()
                }
            } else {
                item.body.clone()
            };
            let db_path = self.db_path.clone();
            std::thread::spawn(move || {
                embeddings::ensure_extension_registered();
                match rusqlite::Connection::open(&db_path) {
                    Ok(bg_conn) => {
                        let _ = bg_conn.busy_timeout(std::time::Duration::from_secs(5));
                        match embeddings::index(&bg_conn, &id_clone, &embed_text) {
                            Ok(()) => log::info!("[mem] indexed embedding for {}", id_clone),
                            Err(e) => log::warn!("[mem] bg embedding index failed for {}: {}", id_clone, e),
                        }
                    }
                    Err(e) => log::warn!("[mem] bg conn open failed: {}", e),
                }
            });
        }
        // Auto-extract all 5 categories from free-form text (regex, sync, fast)
        if matches!(
            item.kind,
            MemKind::Conversation | MemKind::Note | MemKind::Daily | MemKind::Learning | MemKind::Fact
        ) {
            let extracted = extract::extract_all(&item.body);
            for ext in extracted {
                let child_kind = match ext.kind {
                    extract::ExtractKind::Preference => MemKind::Preference,
                    extract::ExtractKind::Decision => MemKind::Fact,
                    extract::ExtractKind::Milestone => MemKind::Fact,
                    extract::ExtractKind::Problem => MemKind::Learning,
                    extract::ExtractKind::Emotional => MemKind::Note,
                };
                let body = format!("User has mentioned ({}): {}", ext.label, ext.captured);
                let tags: Vec<String> = ext.kind.tag().split(',').map(|s| s.to_string()).collect();
                let child = NewMemItem {
                    kind: child_kind,
                    scope: item.scope.clone(),
                    title: format!("auto: {}", &body.chars().take(50).collect::<String>()),
                    summary: Some(body.clone()),
                    area: item.area.clone(),
                    tags,
                    body,
                    source: Some(format!("extracted_from:{}", id)),
                    importance: 2.0, // low importance so originals rank above auto-extracted
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

    /// Batch delete all items matching an area. Returns count of deleted items.
    /// Cleans up vectors, relations, and FTS entries for each.
    pub fn delete_by_area(&self, area: &str) -> Result<usize, String> {
        let conn = self.conn.lock();
        let ids: Vec<String> = {
            let mut out = Vec::new();
            if let Ok(mut stmt) = conn.prepare("SELECT id FROM mem_items WHERE area = ?1") {
                if let Ok(rows) = stmt.query_map(rusqlite::params![area], |r| r.get::<_, String>(0)) {
                    for row in rows.flatten() {
                        out.push(row);
                    }
                }
            }
            out
        };
        let mut count = 0;
        for id in &ids {
            if store::delete(&conn, id)? {
                count += 1;
            }
        }
        // Also remove the area entity
        let _ = conn.execute(
            "DELETE FROM entities WHERE id = ?1",
            rusqlite::params![format!("area_{}", area)],
        );
        Ok(count)
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
        graph::upsert_relation(&conn, r)
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

    /// Invalidate an open relation — set valid_to without deleting.
    pub fn graph_invalidate(
        &self,
        from_id: &str,
        relation_type: &str,
        to_id: &str,
        ended: Option<&str>,
    ) -> Result<bool, String> {
        let conn = self.conn.lock();
        graph::invalidate_relation(&conn, from_id, relation_type, to_id, ended)
    }

    /// Full temporal timeline for an entity (both directions, all relations).
    pub fn graph_timeline(&self, entity_id: &str) -> Result<Vec<Relation>, String> {
        let conn = self.conn.lock();
        graph::timeline(&conn, entity_id)
    }

    /// BFS traversal from an entity up to max_hops. Caps nodes to prevent explosion.
    pub fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
    ) -> Result<graph::TraversalResult, String> {
        let conn = self.conn.lock();
        graph::traverse(&conn, start, max_hops, max_nodes, as_of)
    }

    /// Shortest path between two entities (BFS).
    pub fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
    ) -> Result<Option<Vec<Relation>>, String> {
        let conn = self.conn.lock();
        graph::shortest_path(&conn, from, to, max_hops)
    }

    /// Tunnels: entities bridging 2+ areas (cross-project connections).
    pub fn graph_tunnels(&self) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock();
        graph::tunnels(&conn)
    }

    /// Create an alias (merges the alias entity into canonical).
    pub fn graph_alias(&self, alias_id: &str, canonical_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::add_alias(&conn, alias_id, canonical_id)
    }

    /// Resolve an entity id through the aliases table.
    pub fn graph_resolve(&self, id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        graph::resolve_canonical(&conn, id)
    }

    /// List aliases for a canonical entity.
    pub fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String> {
        let conn = self.conn.lock();
        graph::list_aliases(&conn, canonical_id)
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

    /// Full graph dump for visualization frontend.
    pub fn graph_full(&self) -> Result<store::GraphDump, String> {
        let conn = self.conn.lock();
        store::graph_full(&conn)
    }

    /// Batch write force-layout positions. Returns number of rows updated.
    pub fn set_positions(
        &self,
        positions: &[(String, f64, f64)],
    ) -> Result<usize, String> {
        let mut conn = self.conn.lock();
        store::set_positions(&mut conn, positions)
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
