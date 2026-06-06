//! Mem graph operations + learning promotion.

use super::graph_repair::{
    augment_entity, delete_relations_by_rowids, find_duplicate_entities, find_duplicate_relations,
    merge_relations, DuplicateEntityGroup, DuplicateRelationGroup, EntitySimilarityStrategy,
    MergeReport, RelationColumn,
};
use super::{graph, store, Entity, MemManager, Relation};

impl MemManager {
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
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        let conn = self.conn.lock();
        graph::related(&conn, id, rel, as_of, include_invalidated)
    }

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

    pub fn graph_timeline(
        &self,
        entity_id: &str,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        let conn = self.conn.lock();
        graph::timeline(&conn, entity_id, include_invalidated)
    }

    pub fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<graph::TraversalResult, String> {
        let conn = self.conn.lock();
        graph::traverse(
            &conn,
            start,
            max_hops,
            max_nodes,
            as_of,
            include_invalidated,
        )
    }

    /// Shortest path between two entities (BFS).
    pub fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
        include_invalidated: bool,
    ) -> Result<Option<Vec<Relation>>, String> {
        let conn = self.conn.lock();
        graph::shortest_path(&conn, from, to, max_hops, include_invalidated)
    }

    pub fn graph_tunnels(
        &self,
        include_invalidated: bool,
    ) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock();
        graph::tunnels(&conn, include_invalidated)
    }

    pub fn graph_alias(&self, alias_id: &str, canonical_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::add_alias(&conn, alias_id, canonical_id)
    }

    pub fn graph_resolve(&self, id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        graph::resolve_canonical(&conn, id)
    }

    pub fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String> {
        let conn = self.conn.lock();
        graph::list_aliases(&conn, canonical_id)
    }

    pub fn promote_learning(&self, id: &str) -> Result<(), String> {
        let item = self.get(id)?.ok_or_else(|| format!("not found: {}", id))?;
        if item.kind != "learning" {
            return Err(format!(
                "can only promote kind=learning, got kind={}",
                item.kind
            ));
        }
        let old_importance = item.importance;
        let new_importance = old_importance.max(4.0);
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), id],
        ).map_err(|e| format!("promote: {}", e))?;
        store::history_log(
            &conn,
            id,
            "promote",
            &store::HistorySnapshot {
                title: None,
                body: None,
                importance: Some(old_importance),
                kind: Some("learning".into()),
            },
            &store::HistorySnapshot {
                title: None,
                body: None,
                importance: Some(new_importance),
                kind: Some("fact".into()),
            },
            None,
        );
        Ok(())
    }

    pub fn graph_find_duplicate_relations(
        &self,
        group_by: &[RelationColumn],
        limit: usize,
    ) -> Result<Vec<DuplicateRelationGroup>, String> {
        let conn = self.conn.lock();
        find_duplicate_relations(&conn, group_by, limit)
    }

    pub fn graph_delete_relations(&self, rowids: &[i64], actor: &str) -> Result<u64, String> {
        let conn = self.conn.lock();
        delete_relations_by_rowids(&conn, rowids, actor)
    }

    pub fn graph_delete_entity(
        &self,
        entity_id: &str,
        force: bool,
        purge_history: bool,
        actor: &str,
    ) -> Result<crate::mem::graph_repair::DeleteEntityReport, String> {
        let conn = self.conn.lock();
        crate::mem::graph_repair::delete_entity_by_id(&conn, entity_id, force, purge_history, actor)
    }

    pub fn graph_repair_orphan_vectors(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::mem::graph_repair::RepairOrphanVectorsReport, String> {
        let conn = self.conn.lock();
        crate::mem::graph_repair::repair_orphan_vectors(&conn, dry_run, actor)
    }

    pub fn graph_repair_orphan_blobs(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::mem::graph_repair::RepairOrphanBlobsReport, String> {
        let conn = self.conn.lock();
        crate::mem::graph_repair::repair_orphan_blobs(&conn, self.blobs_dir(), dry_run, actor)
    }

    pub fn graph_merge_relations(
        &self,
        keep: i64,
        merge: &[i64],
        actor: &str,
    ) -> Result<MergeReport, String> {
        let conn = self.conn.lock();
        merge_relations(&conn, keep, merge, actor)
    }

    pub fn graph_find_duplicate_entities(
        &self,
        strategy: EntitySimilarityStrategy,
        limit: usize,
    ) -> Result<Vec<DuplicateEntityGroup>, String> {
        let conn = self.conn.lock();
        find_duplicate_entities(&conn, strategy, limit)
    }

    pub fn graph_augment_entity(
        &self,
        entity_id: &str,
        patch: serde_json::Map<String, serde_json::Value>,
        actor: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        augment_entity(&conn, entity_id, patch, actor)
    }
}
