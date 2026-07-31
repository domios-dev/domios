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
            kind: q.kind.clone(),
            scope: q.scope.clone(),
            limit: q.limit,
            ..Default::default()
        };
        search::hybrid_rrf(conn, &sq, blobs_dir).map(|hits| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::{MemKind, MemScope, NewMemItem};

    fn insert_search_item(
        conn: &Connection,
        blobs: &Path,
        kind: MemKind,
        scope: MemScope,
        title: String,
    ) {
        store::insert(
            conn,
            &NewMemItem {
                kind,
                scope,
                title,
                summary: None,
                area: None,
                tags: vec![],
                body: "shared backend filtering proof".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            blobs,
        )
        .unwrap();
    }

    #[test]
    fn plan_search_includes_semantic_only_hits() {
        let port = MemSearchPort;
        port.ensure_embedding_registered();
        let conn = Connection::open_in_memory().unwrap();
        port.init_embedding_ext(&conn);
        port.init_schema(&conn).unwrap();
        let blobs = tempfile::tempdir().unwrap();
        let body = "Vehicle repair mechanics diagnose engines, brakes, tires, and transmissions.";
        let id = store::insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "Workshop diagnostics".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: body.into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            blobs.path(),
        )
        .unwrap();
        port.embed(&conn, &id, body).unwrap();
        let query = PlanIndexQuery {
            query: "automobile maintenance".into(),
            kind: None,
            scope: None,
            limit: Some(5),
        };

        assert!(search::run(
            &conn,
            &search::SearchQuery {
                query: query.query.clone(),
                limit: query.limit,
                ..Default::default()
            },
            blobs.path(),
        )
        .unwrap()
        .is_empty());
        assert!(port
            .run_search(&conn, &query, blobs.path())
            .unwrap()
            .iter()
            .any(|hit| hit.id == id));
    }

    #[test]
    fn plan_search_kind_filter() {
        let port = MemSearchPort;
        let conn = Connection::open_in_memory().unwrap();
        port.init_schema(&conn).unwrap();
        let blobs = tempfile::tempdir().unwrap();

        for index in 0..15 {
            insert_search_item(
                &conn,
                blobs.path(),
                MemKind::Task,
                MemScope::Global,
                format!("Searchable task {index}"),
            );
        }
        for index in 0..15 {
            insert_search_item(
                &conn,
                blobs.path(),
                MemKind::SpecSection,
                MemScope::Global,
                format!("Searchable spec section {index}"),
            );
        }
        insert_search_item(
            &conn,
            blobs.path(),
            MemKind::Task,
            MemScope::Workspace("ws_1".into()),
            "Workspace task".into(),
        );

        let unfiltered = port
            .run_search(
                &conn,
                &PlanIndexQuery {
                    query: "shared backend filtering proof".into(),
                    kind: None,
                    scope: None,
                    limit: Some(40),
                },
                blobs.path(),
            )
            .unwrap();
        assert!(unfiltered.iter().any(|hit| hit.kind == "task"));
        assert!(unfiltered.iter().any(|hit| hit.kind == "spec_section"));

        let hits = port
            .run_search(
                &conn,
                &PlanIndexQuery {
                    query: "shared backend filtering proof".into(),
                    kind: Some("task".into()),
                    scope: None,
                    limit: Some(10),
                },
                blobs.path(),
            )
            .unwrap();

        assert_eq!(hits.len(), 10);
        assert!(hits.iter().all(|hit| hit.kind == "task"));

        let scoped = port
            .run_search(
                &conn,
                &PlanIndexQuery {
                    query: "shared backend filtering proof".into(),
                    kind: None,
                    scope: Some("workspace:ws_1".into()),
                    limit: Some(10),
                },
                blobs.path(),
            )
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].scope, "workspace:ws_1");
    }
}
