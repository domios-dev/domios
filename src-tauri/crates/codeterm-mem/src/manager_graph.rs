//! Mem graph operations + learning promotion.

use super::graph_repair::{
    DuplicateEntityGroup, DuplicateRelationGroup, EntitySimilarityStrategy, MergeReport,
    RelationColumn,
};
use super::{graph, Entity, MemManager, Relation};

impl MemManager {
    pub fn graph_create_entity(&self, e: &Entity) -> Result<(), String> {
        self.guard_commit()?;
        self.store.graph_create_entity(&Self::write_context(), e)?;
        self.enqueue_entity_embedding(&e.id);
        Ok(())
    }

    pub fn graph_relate(&self, r: &Relation) -> Result<(), String> {
        self.guard_commit()?;
        self.store.graph_relate(&Self::write_context(), r)
    }

    pub fn graph_related(
        &self,
        id: &str,
        rel: Option<&str>,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        self.store
            .graph_related(id, rel, as_of, include_invalidated)
    }

    pub fn graph_invalidate(
        &self,
        from_id: &str,
        relation_type: &str,
        to_id: &str,
        ended: Option<&str>,
    ) -> Result<bool, String> {
        self.store
            .graph_invalidate(&Self::write_context(), from_id, relation_type, to_id, ended)
    }

    pub fn graph_timeline(
        &self,
        entity_id: &str,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        self.store.graph_timeline(entity_id, include_invalidated)
    }

    pub fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<graph::TraversalResult, String> {
        self.store
            .graph_traverse(start, max_hops, max_nodes, as_of, include_invalidated)
    }

    /// Shortest path between two entities (BFS).
    pub fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
        include_invalidated: bool,
    ) -> Result<Option<Vec<Relation>>, String> {
        self.store
            .graph_shortest_path(from, to, max_hops, include_invalidated)
    }

    pub fn graph_tunnels(
        &self,
        include_invalidated: bool,
    ) -> Result<Vec<serde_json::Value>, String> {
        self.store.graph_tunnels(include_invalidated)
    }

    pub fn graph_alias(&self, alias_id: &str, canonical_id: &str) -> Result<(), String> {
        self.store
            .graph_alias(&Self::write_context(), alias_id, canonical_id)
    }

    pub fn graph_resolve(&self, id: &str) -> Result<String, String> {
        self.store.graph_resolve(id)
    }

    pub fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String> {
        self.store.graph_aliases(canonical_id)
    }

    pub fn promote_learning(&self, id: &str) -> Result<(), String> {
        self.store.promote_learning(&Self::write_context(), id)
    }

    pub fn graph_find_duplicate_relations(
        &self,
        group_by: &[RelationColumn],
        limit: usize,
    ) -> Result<Vec<DuplicateRelationGroup>, String> {
        self.store.graph_find_duplicate_relations(group_by, limit)
    }

    pub fn graph_delete_relations(&self, rowids: &[i64], actor: &str) -> Result<u64, String> {
        self.store
            .graph_delete_relations(&Self::write_context(), rowids, actor)
    }

    pub fn graph_delete_entity(
        &self,
        entity_id: &str,
        force: bool,
        purge_history: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::DeleteEntityReport, String> {
        self.store.graph_delete_entity(
            &Self::write_context(),
            entity_id,
            force,
            purge_history,
            actor,
        )
    }

    pub fn graph_repair_orphan_vectors(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        self.store
            .graph_repair_orphan_vectors(&Self::write_context(), dry_run, actor)
    }

    pub fn graph_prune_thin_vectors(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        self.store
            .graph_prune_thin_vectors(&Self::write_context(), dry_run, actor)
    }

    pub fn graph_collapse_synonym_edges(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        self.store
            .graph_collapse_synonym_edges(&Self::write_context(), dry_run, actor)
    }

    pub fn graph_prune_dream_hub_edges(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        self.store
            .graph_prune_dream_hub_edges(&Self::write_context(), dry_run, actor)
    }

    pub fn graph_repair_orphan_blobs(
        &self,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanBlobsReport, String> {
        self.store
            .graph_repair_orphan_blobs(&Self::write_context(), dry_run, actor)
    }

    pub fn graph_merge_relations(
        &self,
        keep: i64,
        merge: &[i64],
        actor: &str,
    ) -> Result<MergeReport, String> {
        self.store
            .graph_merge_relations(&Self::write_context(), keep, merge, actor)
    }

    pub fn graph_find_duplicate_entities(
        &self,
        strategy: EntitySimilarityStrategy,
        limit: usize,
    ) -> Result<Vec<DuplicateEntityGroup>, String> {
        self.store.graph_find_duplicate_entities(strategy, limit)
    }

    pub fn graph_augment_entity(
        &self,
        entity_id: &str,
        patch: serde_json::Map<String, serde_json::Value>,
        actor: &str,
    ) -> Result<(), String> {
        self.store
            .graph_augment_entity(&Self::write_context(), entity_id, patch, actor)?;
        self.enqueue_entity_embedding(entity_id);
        Ok(())
    }
    fn enqueue_entity_embedding(&self, id: &str) {
        if let Some(text) = self.store.entity_embed_text(id) {
            self.embed_queue.enqueue(id, text);
        }
    }
}
