//! Mem search and recall operations.

use super::{embeddings, layers, rerank, search, MemManager, SearchHit, SearchQuery, WakeUpOptions};

impl MemManager {
    /// Hybrid recall: RRF fusion of FTS5 + vector ANN results.
    /// Always runs both backends and merges via Reciprocal Rank Fusion
    /// for maximum recall quality (typo-tolerant + keyword-precise).
    /// Falls back to FTS5-only when embeddings feature is off.
    pub fn recall(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::hybrid_rrf(&conn, q, &self.blobs_dir)
    }

    /// A/B-testable hybrid recall with custom ranking config.
    pub fn recall_with_config(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::hybrid_rrf_with_config(&conn, q, &self.blobs_dir, cfg)
    }

    pub fn vector_recall(&self, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
        let conn = self.conn.lock();
        embeddings::recall(&conn, query, k)
    }

    pub fn index_embedding(&self, id: &str, body: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        embeddings::index(&conn, id, body)
    }

    /// Bulk re-embed mem_items and/or graph entities. Used after embedder
    /// model swap, or to backfill entities that pre-date the auto-embed-on-
    /// create wiring (everything before v1.1.23 if the bug fix landed there).
    pub fn reembed_bulk(
        &self,
        do_items: bool,
        do_entities: bool,
    ) -> Result<ReembedReport, String> {
        let conn = self.conn.lock();
        let mut report = ReembedReport::default();

        if do_items {
            let rows: Vec<(String, String)> = {
                let mut stmt = conn
                    .prepare("SELECT id, COALESCE(body_inline, '') FROM mem_items")
                    .map_err(|e| e.to_string())?;
                let mapped = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for row in mapped {
                    out.push(row.map_err(|e| e.to_string())?);
                }
                out
            };
            for (id, body) in rows {
                if body.trim().is_empty() {
                    continue;
                }
                match embeddings::index(&conn, &id, &body) {
                    Ok(_) => report.items += 1,
                    Err(e) => report.errors.push(format!("item {}: {}", id, e)),
                }
            }
        }

        if do_entities {
            let rows: Vec<(String, String, String)> = {
                let mut stmt = conn
                    .prepare("SELECT id, entity_type, properties_json FROM entities")
                    .map_err(|e| e.to_string())?;
                let mapped = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for row in mapped {
                    out.push(row.map_err(|e| e.to_string())?);
                }
                out
            };
            for (id, entity_type, props_json) in rows {
                let text = embeddings::embeddable_text_for_entity(&id, &entity_type, &props_json);
                match embeddings::index(&conn, &id, &text) {
                    Ok(_) => report.entities += 1,
                    Err(e) => report.errors.push(format!("entity {}: {}", id, e)),
                }
            }
        }

        Ok(report)
    }

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::run(&conn, q, &self.blobs_dir)
    }

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
}

#[derive(Default, Debug, Clone)]
pub struct ReembedReport {
    pub items: u64,
    pub entities: u64,
    pub errors: Vec<String>,
}
