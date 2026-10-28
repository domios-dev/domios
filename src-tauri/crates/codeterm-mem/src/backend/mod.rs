use crate::embedder::{EmbedderIdentity, StampedVector};
use crate::graph_repair::{
    DuplicateEntityGroup, DuplicateRelationGroup, EntitySimilarityStrategy, MergeReport,
    RelationColumn,
};
use crate::store::MemAuthor;
use crate::store::{GraphDump, HistoryEntry, MemUpdate, PinnedPosition, TailEntry};
use crate::{
    graph, search, store, Entity, MemItem, MemStats, NewMemItem, Relation, SearchHit, SearchQuery,
    WakeUpOptions,
};
pub mod blueprint;
#[cfg(any(test, feature = "test-support"))]
pub mod contract;
pub mod engines;
pub mod sqlite;
pub use blueprint::BlueprintStore;
mod transfer;
pub use engines::EngineStore;
pub use sqlite::SqliteMemStore;
pub use transfer::{ExportCursor, ExportPage, StoreRow, StoreTable, StoreValue, TableCounts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteContext {
    pub author: MemAuthor,
    pub changed_by: String,
    pub replace_guard: Option<store::ReplaceGuard>,
    use_system_default: bool,
}

impl WriteContext {
    pub fn new(author: MemAuthor, changed_by: impl Into<String>) -> Self {
        Self {
            author,
            changed_by: changed_by.into(),
            use_system_default: false,
            replace_guard: None,
        }
    }

    pub(crate) fn unscoped(changed_by: String) -> Self {
        Self {
            author: MemAuthor::default(),
            changed_by,
            use_system_default: true,
            replace_guard: None,
        }
    }

    pub fn default_system_author(&self, label: &str) -> MemAuthor {
        if self.use_system_default {
            MemAuthor::system(label)
        } else {
            self.author.clone()
        }
    }

    pub fn with_author(&self, author: MemAuthor) -> Self {
        let mut next = self.clone();
        next.author = author;
        next.use_system_default = false;
        next
    }

    pub fn with_changed_by(&self, changed_by: impl Into<String>) -> Self {
        let mut next = self.clone();
        next.changed_by = changed_by.into();
        next
    }

    pub fn system(label: &str) -> Self {
        Self::new(MemAuthor::system(label), label)
    }
}

pub trait MemStore: BlueprintStore + EngineStore + Send + Sync {
    fn seed_graph(
        &self,
        ctx: &WriteContext,
        entities: &[(Entity, String)],
        relations: &[Relation],
    ) -> Result<(), String>;

    fn deletion_targets(&self, id: &str) -> Result<(Vec<String>, Option<String>), String>;
    fn ids_by_area(&self, area: &str) -> Vec<String>;
    fn delete_batch(&self, ctx: &WriteContext, ids: &[String]) -> Result<Deletion, String>;

    fn recluster_prompt(&self) -> Result<Option<String>, String>;
    fn recluster_finalize(
        &self,
        response: &str,
        model: &str,
        plan_dir: &std::path::Path,
    ) -> Result<crate::dreaming::recluster::ReclusterReport, String>;

    fn sync_status(&self, timeout: std::time::Duration) -> Option<serde_json::Value>;
    #[cfg(any(test, feature = "test-support"))]
    fn fixture_set_kind(&self, ctx: &WriteContext, id: &str, kind: &str) -> Result<(), String>;
    #[cfg(any(test, feature = "test-support"))]
    fn lock_for_test(&self) -> Box<dyn StorageLock + '_>;

    fn export_page(&self, after: Option<&ExportCursor>, limit: usize)
        -> Result<ExportPage, String>;
    fn import_page(&self, ctx: &WriteContext, page: &ExportPage) -> Result<(), String>;
    fn table_counts(&self) -> Result<TableCounts, String>;

    fn promote_learning(&self, ctx: &WriteContext, id: &str) -> Result<(), String>;
    fn graph_find_duplicate_relations(
        &self,
        group_by: &[RelationColumn],
        limit: usize,
    ) -> Result<Vec<DuplicateRelationGroup>, String>;
    fn graph_delete_relations(
        &self,
        ctx: &WriteContext,
        rowids: &[i64],
        actor: &str,
    ) -> Result<u64, String>;
    fn graph_delete_entity(
        &self,
        ctx: &WriteContext,
        entity_id: &str,
        force: bool,
        purge_history: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::DeleteEntityReport, String>;
    fn graph_repair_orphan_vectors(
        &self,
        ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String>;
    fn graph_prune_thin_vectors(
        &self,
        ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String>;
    fn graph_collapse_synonym_edges(
        &self,
        ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String>;
    fn graph_prune_dream_hub_edges(
        &self,
        ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String>;
    fn graph_repair_orphan_blobs(
        &self,
        ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanBlobsReport, String>;
    fn graph_merge_relations(
        &self,
        ctx: &WriteContext,
        keep: i64,
        merge: &[i64],
        actor: &str,
    ) -> Result<MergeReport, String>;
    fn graph_find_duplicate_entities(
        &self,
        strategy: EntitySimilarityStrategy,
        limit: usize,
    ) -> Result<Vec<DuplicateEntityGroup>, String>;
    fn graph_augment_entity(
        &self,
        ctx: &WriteContext,
        entity_id: &str,
        patch: serde_json::Map<String, serde_json::Value>,
        actor: &str,
    ) -> Result<(), String>;
    fn entity_embed_text(&self, id: &str) -> Option<String>;
    fn insert(&self, ctx: &WriteContext, item: &NewMemItem) -> Result<String, String>;
    fn insert_seeded(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        fingerprint: &str,
    ) -> Result<String, String>;
    fn get(&self, id: &str) -> Result<Option<MemItem>, String>;
    fn update(&self, ctx: &WriteContext, id: &str, patch: &MemUpdate) -> Result<bool, String>;
    fn delete(&self, ctx: &WriteContext, id: &str) -> Result<bool, String>;
    fn managed_payload(&self, id: &str) -> Result<Option<(NewMemItem, Option<String>)>, String>;
    fn cas_refresh_bundled(
        &self,
        ctx: &WriteContext,
        id: &str,
        expected_fingerprint: Option<&str>,
        new_item: &NewMemItem,
        new_fingerprint: &str,
    ) -> Result<bool, String>;
    fn bootstrap_seed_fingerprint(
        &self,
        ctx: &WriteContext,
        id: &str,
        expected: &NewMemItem,
        fingerprint: &str,
    ) -> Result<bool, String>;
    fn graph_full(&self) -> Result<GraphDump, String>;
    fn list_pinned_positions(&self) -> Result<Vec<PinnedPosition>, String>;
    fn list_pinned(&self, limit: usize) -> Result<Vec<MemItem>, String>;
    fn list_by_source_prefix(&self, kind: &str, prefix: &str) -> Result<Vec<MemItem>, String>;
    fn set_pinned_position(
        &self,
        ctx: &WriteContext,
        mem_id: &str,
        x: f64,
        y: f64,
    ) -> Result<bool, String>;
    fn delete_pinned_position(&self, ctx: &WriteContext, mem_id: &str) -> Result<usize, String>;
    fn stats(&self) -> Result<MemStats, String>;
    fn get_item_history(&self, id: &str) -> Result<Vec<HistoryEntry>, String>;
    fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, u64>>, String>;
    fn tail_recent(&self, since: Option<&str>, limit: usize) -> Result<Vec<TailEntry>, String>;
    fn dream_diaries_since(&self, since_ms: u64) -> Result<Vec<String>, String>;
    fn graph_create_entity(&self, ctx: &WriteContext, e: &Entity) -> Result<(), String>;
    fn graph_relate(&self, ctx: &WriteContext, r: &Relation) -> Result<(), String>;
    fn graph_insert_relation(&self, ctx: &WriteContext, r: &Relation) -> Result<(), String>;
    fn graph_related(
        &self,
        id: &str,
        rel: Option<&str>,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String>;
    fn graph_invalidate(
        &self,
        ctx: &WriteContext,
        from_id: &str,
        relation_type: &str,
        to_id: &str,
        ended: Option<&str>,
    ) -> Result<bool, String>;
    fn graph_timeline(
        &self,
        entity_id: &str,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String>;
    fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<graph::TraversalResult, String>;
    fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
        include_invalidated: bool,
    ) -> Result<Option<Vec<Relation>>, String>;
    fn graph_tunnels(&self, include_invalidated: bool) -> Result<Vec<serde_json::Value>, String>;
    fn graph_alias(
        &self,
        ctx: &WriteContext,
        alias_id: &str,
        canonical_id: &str,
    ) -> Result<(), String>;
    fn graph_resolve(&self, id: &str) -> Result<String, String>;
    fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String>;
    fn search(&self, q: &SearchQuery, record_access: bool) -> Result<Vec<SearchHit>, String>;
    fn recall(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
        embedder: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
        record_access: bool,
    ) -> Result<Vec<SearchHit>, String>;
    fn vector_recall(
        &self,
        query: &str,
        k: usize,
        embedder: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
    ) -> Result<Vec<(String, f64)>, String>;
    fn vector_stamp(&self) -> Result<Option<EmbedderIdentity>, String>;
    fn init_vectors(&self, ctx: &WriteContext, identity: &EmbedderIdentity) -> Result<(), String>;
    fn insert_vector(
        &self,
        ctx: &WriteContext,
        id: &str,
        chunk: usize,
        vector: &StampedVector,
    ) -> Result<crate::embedder::index::InsertOutcome, String>;
    fn vector_search(&self, vector: &StampedVector, k: usize)
        -> Result<Vec<(String, f64)>, String>;
    fn store_vectors(
        &self,
        ctx: &WriteContext,
        id: &str,
        vectors: Vec<Result<StampedVector, crate::embedder::EmbedError>>,
    ) -> Result<(), String>;
    fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String>;
    fn merge_area(
        &self,
        ctx: &WriteContext,
        from: &str,
        into: &str,
        dry_run: bool,
        dream_run: Option<&str>,
    ) -> Result<store::area_merge::AreaMergeOutcome, String>;
    fn restore_area(
        &self,
        ctx: &WriteContext,
        id: &str,
        version: Option<i64>,
    ) -> Result<Option<String>, String>;
    fn calibration_sweep(
        &self,
        ctx: &WriteContext,
        force: bool,
    ) -> Result<serde_json::Value, String>;
    fn calibration_log(
        &self,
        since: &str,
        limit: usize,
        count_only: bool,
    ) -> Result<serde_json::Value, String>;
    fn nightly_calibration(
        &self,
        ctx: &WriteContext,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, String>;
    fn stale(
        &self,
        ctx: &WriteContext,
        p: &crate::curation::calibration::stale::StaleParams,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
    ) -> Result<serde_json::Value, String>;
    fn restore_kind(
        &self,
        ctx: &WriteContext,
        id: &str,
        restore: &crate::curation::merge::KindRestore,
        body: Option<&str>,
        actor: &str,
    ) -> Result<serde_json::Value, String>;
    fn merge_items(
        &self,
        ctx: &WriteContext,
        req: &crate::curation::merge::MergeRequest<'_>,
        actor: &str,
        merge_cap: u32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, crate::curation::merge::MergeError>;
    fn save(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        replace: bool,
    ) -> Result<crate::manager_crud::SaveOutcome, SaveError>;
    fn archive(
        &self,
        ctx: &WriteContext,
        id: &str,
        reason: &str,
        actor: &str,
    ) -> Result<serde_json::Value, crate::curation::merge::MergeError>;
    fn is_pinned(&self, id: &str) -> Result<bool, String>;
    fn diary_page(&self, offset: usize, limit: usize) -> Result<serde_json::Value, String>;
    fn bump_on_get(&self, ctx: &WriteContext, kind: &str, id: &str);
    fn graph_type_counts(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>);
    fn access_count(&self, id: &str) -> Result<Option<i64>, String>;
    fn record_access(&self, ctx: &WriteContext, ids: &[String]) -> Result<(), String>;
    fn graph_candidates(&self, q: &SearchQuery, m: usize) -> Result<Vec<SearchHit>, String>;
}

#[derive(Debug)]
pub enum SaveError {
    Storage(String),
    Guard(store::ReplaceGuardError),
}
impl From<String> for SaveError {
    fn from(error: String) -> Self {
        Self::Storage(error)
    }
}
impl From<store::ReplaceGuardError> for SaveError {
    fn from(error: store::ReplaceGuardError) -> Self {
        Self::Guard(error)
    }
}
impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => f.write_str(error),
            Self::Guard(store::ReplaceGuardError::AreaNotAllowed) => {
                f.write_str("neighbor_memory_area_not_granted")
            }
            Self::Guard(store::ReplaceGuardError::TitleConflict) => {
                f.write_str("neighbor_memory_title_conflict")
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub trait StorageLock {}
#[cfg(any(test, feature = "test-support"))]
impl<T> StorageLock for parking_lot::MutexGuard<'_, T> {}

#[derive(Default)]
pub struct Deletion {
    pub ids: Vec<String>,
    pub library_areas: Vec<String>,
    pub skill_sources: Vec<(String, Option<String>)>,
}
