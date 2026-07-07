use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::ports::{PlanIndexHit, PlanIndexQuery, PlanSearchPort};
use crate::mem::{embeddings, search, store};

pub struct MemSearchPort;

impl PlanSearchPort for MemSearchPort {
    fn data_dir(&self) -> PathBuf {
        crate::paths::data_dir()
    }

    fn ensure_embedding_registered(&self) {
        embeddings::ensure_extension_registered();
    }

    fn init_embedding_ext(&self, conn: &Connection) {
        let _ = embeddings::init_extension(conn);
    }

    fn init_schema(&self, conn: &Connection) -> Result<(), String> {
        store::init_schema(conn).map_err(|e| e.to_string())
    }

    fn embed(&self, conn: &Connection, id: &str, body: &str) -> Result<(), String> {
        embeddings::index(conn, id, body)
    }

    fn run_search(
        &self,
        conn: &Connection,
        q: &PlanIndexQuery,
        blobs_dir: &Path,
    ) -> Result<Vec<PlanIndexHit>, String> {
        let sq = search::SearchQuery {
            query: q.query.clone(),
            scope: q.scope.clone(),
            limit: q.limit,
            ..Default::default()
        };
        search::run(conn, &sq, blobs_dir).map(|hits| {
            hits.into_iter()
                .map(|h| PlanIndexHit {
                    score: h.score,
                    id: h.item.id,
                    kind: h.item.kind,
                    title: h.item.title,
                    body: h.item.body,
                    scope: h.item.scope,
                    source: h.item.source,
                    updated_at: h.item.updated_at,
                })
                .collect()
        })
    }
}
