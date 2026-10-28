use super::{operations, SqliteMemStore};
use crate::backend::MemStore;
use crate::backend::WriteContext;
use crate::embedder::{EmbedderIdentity, StampedVector};
use crate::graph_repair::{
    augment_entity, delete_relations_by_rowids, find_duplicate_entities, find_duplicate_relations,
    merge_relations, DuplicateEntityGroup, DuplicateRelationGroup, EntitySimilarityStrategy,
    MergeReport, RelationColumn,
};
use crate::store::{GraphDump, HistoryEntry, MemUpdate, PinnedPosition, TailEntry};
use crate::{
    embeddings, graph, layers, search, store, Entity, MemItem, MemStats, NewMemItem, Relation,
    SearchHit, SearchQuery, WakeUpOptions,
};
impl MemStore for SqliteMemStore {
    fn seed_graph(
        &self,
        _ctx: &WriteContext,
        entities: &[(Entity, String)],
        relations: &[Relation],
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        for (entity, at) in entities {
            conn.execute("INSERT OR IGNORE INTO entities (id, entity_type, scope, properties_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                rusqlite::params![entity.id, entity.entity_type, entity.scope, entity.properties.to_string(), at])
                .map_err(|e| format!("seed entity {}: {e}", entity.id))?;
        }
        for relation in relations {
            graph::insert_relation(&conn, relation)?;
        }
        Ok(())
    }

    fn deletion_targets(&self, id: &str) -> Result<(Vec<String>, Option<String>), String> {
        self.read_deletion_targets(id)
    }
    fn ids_by_area(&self, area: &str) -> Vec<String> {
        self.read_area_ids(area)
    }
    fn delete_batch(
        &self,
        ctx: &WriteContext,
        ids: &[String],
    ) -> Result<crate::backend::Deletion, String> {
        self.remove_batch(ctx, ids)
    }

    fn recluster_prompt(&self) -> Result<Option<String>, String> {
        crate::dreaming::recluster::storage_prompt(&self.conn.lock())
    }
    fn recluster_finalize(
        &self,
        response: &str,
        model: &str,
        plan_dir: &std::path::Path,
    ) -> Result<crate::dreaming::recluster::ReclusterReport, String> {
        crate::dreaming::recluster::storage_finalize(&self.conn.lock(), response, model, plan_dir)
    }

    #[cfg(any(test, feature = "test-support"))]
    fn fixture_set_kind(&self, _ctx: &WriteContext, id: &str, kind: &str) -> Result<(), String> {
        self.conn
            .lock()
            .execute("UPDATE mem_items SET kind=?2 WHERE id=?1", [id, kind])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(any(test, feature = "test-support"))]
    fn lock_for_test(&self) -> Box<dyn crate::backend::StorageLock + '_> {
        Box::new(self.conn.lock())
    }
    fn sync_status(&self, timeout: std::time::Duration) -> Option<serde_json::Value> {
        let conn = self.conn.try_lock_for(timeout)?;
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_items WHERE source_path IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let stale: i64 = conn.query_row("SELECT COUNT(*) FROM mem_items WHERE source_path IS NOT NULL AND tags_json LIKE '%source-missing%'", [], |r| r.get(0)).unwrap_or(0);
        Some(serde_json::json!({ "tracked_files": total, "stale_files": stale }))
    }

    fn export_page(
        &self,
        after: Option<&crate::backend::ExportCursor>,
        limit: usize,
    ) -> Result<crate::backend::ExportPage, String> {
        self.export_rows(after, limit)
    }
    fn import_page(
        &self,
        _ctx: &WriteContext,
        page: &crate::backend::ExportPage,
    ) -> Result<(), String> {
        self.import_rows(page)
    }
    fn table_counts(&self) -> Result<crate::backend::TableCounts, String> {
        self.counts()
    }

    fn promote_learning(&self, ctx: &WriteContext, id: &str) -> Result<(), String> {
        let item = store::get(&self.conn.lock(), id, &self.blobs_dir)?
            .ok_or_else(|| format!("not found: {}", id))?;
        if item.kind != "learning" {
            return Err(format!(
                "can only promote kind=learning, got kind={}",
                item.kind
            ));
        }
        if item.author.is_neighbor() {
            return Err("neighbor content cannot be promoted".into());
        }
        let old_importance = item.importance;
        let new_importance = old_importance.max(4.0);
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), id],
        ).map_err(|e| format!("promote: {}", e))?;
        store::history_log_with_context(
            ctx,
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
    fn graph_find_duplicate_relations(
        &self,
        group_by: &[RelationColumn],
        limit: usize,
    ) -> Result<Vec<DuplicateRelationGroup>, String> {
        let conn = self.conn.lock();
        find_duplicate_relations(&conn, group_by, limit)
    }
    fn graph_delete_relations(
        &self,
        _ctx: &WriteContext,
        rowids: &[i64],
        actor: &str,
    ) -> Result<u64, String> {
        let conn = self.conn.lock();
        delete_relations_by_rowids(&conn, rowids, actor)
    }
    fn graph_delete_entity(
        &self,
        _ctx: &WriteContext,
        entity_id: &str,
        force: bool,
        purge_history: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::DeleteEntityReport, String> {
        let conn = self.conn.lock();
        crate::graph_repair::delete_entity_by_id(&conn, entity_id, force, purge_history, actor)
    }
    fn graph_repair_orphan_vectors(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        let conn = self.conn.lock();
        crate::graph_repair::repair_orphan_vectors(&conn, dry_run, actor)
    }
    fn graph_prune_thin_vectors(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        let conn = self.conn.lock();
        crate::graph_repair::prune_thin_vectors(&conn, dry_run, actor)
    }
    fn graph_collapse_synonym_edges(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        let conn = self.conn.lock();
        crate::graph_repair::collapse_synonym_edges(&conn, dry_run, actor)
    }
    fn graph_prune_dream_hub_edges(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanVectorsReport, String> {
        let conn = self.conn.lock();
        crate::graph_repair::prune_dream_hub_edges(&conn, dry_run, actor)
    }
    fn graph_repair_orphan_blobs(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<crate::graph_repair::RepairOrphanBlobsReport, String> {
        let conn = self.conn.lock();
        crate::graph_repair::repair_orphan_blobs(&conn, &self.blobs_dir, dry_run, actor)
    }
    fn graph_merge_relations(
        &self,
        _ctx: &WriteContext,
        keep: i64,
        merge: &[i64],
        actor: &str,
    ) -> Result<MergeReport, String> {
        let conn = self.conn.lock();
        merge_relations(&conn, keep, merge, actor)
    }
    fn graph_find_duplicate_entities(
        &self,
        strategy: EntitySimilarityStrategy,
        limit: usize,
    ) -> Result<Vec<DuplicateEntityGroup>, String> {
        let conn = self.conn.lock();
        find_duplicate_entities(&conn, strategy, limit)
    }
    fn graph_augment_entity(
        &self,
        _ctx: &WriteContext,
        entity_id: &str,
        patch: serde_json::Map<String, serde_json::Value>,
        actor: &str,
    ) -> Result<(), String> {
        augment_entity(&self.conn.lock(), entity_id, patch, actor)
    }
    fn entity_embed_text(&self, id: &str) -> Option<String> {
        self.conn
            .lock()
            .query_row(
                "SELECT entity_type, properties_json FROM entities WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok()
            .map(|(kind, properties)| {
                crate::embeddings::embeddable_text_for_entity(id, &kind, &properties)
            })
    }
    fn insert(&self, ctx: &WriteContext, item: &NewMemItem) -> Result<String, String> {
        store::insert_with_context(ctx, &self.conn.lock(), item, &self.blobs_dir)
    }
    fn insert_seeded(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        fingerprint: &str,
    ) -> Result<String, String> {
        store::insert_seeded_with_context(
            ctx,
            &self.conn.lock(),
            item,
            &self.blobs_dir,
            fingerprint,
        )
    }
    fn get(&self, id: &str) -> Result<Option<MemItem>, String> {
        store::get(&self.conn.lock(), id, &self.blobs_dir)
    }
    fn update(&self, ctx: &WriteContext, id: &str, patch: &MemUpdate) -> Result<bool, String> {
        store::update_with_context(ctx, &self.conn.lock(), id, patch, &self.blobs_dir)
    }
    fn delete(&self, ctx: &WriteContext, id: &str) -> Result<bool, String> {
        store::delete_with_context(ctx, &self.conn.lock(), id)
    }
    fn managed_payload(&self, id: &str) -> Result<Option<(NewMemItem, Option<String>)>, String> {
        store::managed_payload(&self.conn.lock(), id, &self.blobs_dir)
    }
    fn cas_refresh_bundled(
        &self,
        ctx: &WriteContext,
        id: &str,
        expected_fingerprint: Option<&str>,
        new_item: &NewMemItem,
        new_fingerprint: &str,
    ) -> Result<bool, String> {
        store::cas_refresh_bundled_with_context(
            ctx,
            &self.conn.lock(),
            id,
            &self.blobs_dir,
            expected_fingerprint,
            new_item,
            new_fingerprint,
        )
    }
    fn bootstrap_seed_fingerprint(
        &self,
        _ctx: &WriteContext,
        id: &str,
        expected: &NewMemItem,
        fingerprint: &str,
    ) -> Result<bool, String> {
        store::bootstrap_seed_fingerprint(
            &self.conn.lock(),
            id,
            &self.blobs_dir,
            expected,
            fingerprint,
        )
    }
    fn graph_full(&self) -> Result<GraphDump, String> {
        store::graph_full(&self.conn.lock())
    }
    fn list_pinned_positions(&self) -> Result<Vec<PinnedPosition>, String> {
        store::list_pinned_positions(&self.conn.lock())
    }
    fn list_pinned(&self, limit: usize) -> Result<Vec<MemItem>, String> {
        store::list_pinned(&self.conn.lock(), &self.blobs_dir, limit)
    }
    fn list_by_source_prefix(&self, kind: &str, prefix: &str) -> Result<Vec<MemItem>, String> {
        store::list_by_source_prefix(&self.conn.lock(), &self.blobs_dir, kind, prefix)
    }
    fn set_pinned_position(
        &self,
        _ctx: &WriteContext,
        mem_id: &str,
        x: f64,
        y: f64,
    ) -> Result<bool, String> {
        store::set_pinned_position(&self.conn.lock(), mem_id, x, y)
    }
    fn delete_pinned_position(&self, _ctx: &WriteContext, mem_id: &str) -> Result<usize, String> {
        store::delete_pinned_position(&self.conn.lock(), mem_id)
    }
    fn stats(&self) -> Result<MemStats, String> {
        store::stats(&self.conn.lock())
    }
    fn get_item_history(&self, id: &str) -> Result<Vec<HistoryEntry>, String> {
        store::get_item_history(&self.conn.lock(), id)
    }
    fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, u64>>, String>
    {
        store::get_taxonomy(&self.conn.lock(), workspace)
    }
    fn tail_recent(&self, since: Option<&str>, limit: usize) -> Result<Vec<TailEntry>, String> {
        store::tail_recent(&self.conn.lock(), since, limit)
    }
    fn dream_diaries_since(&self, since_ms: u64) -> Result<Vec<String>, String> {
        store::dream_diaries_since(&self.conn.lock(), since_ms)
    }
    fn graph_create_entity(&self, _ctx: &WriteContext, e: &Entity) -> Result<(), String> {
        graph::insert_entity(&self.conn.lock(), e)
    }
    fn graph_relate(&self, _ctx: &WriteContext, r: &Relation) -> Result<(), String> {
        graph::upsert_relation(&self.conn.lock(), r)
    }
    fn graph_insert_relation(&self, _ctx: &WriteContext, r: &Relation) -> Result<(), String> {
        graph::insert_relation(&self.conn.lock(), r)
    }
    fn graph_related(
        &self,
        id: &str,
        rel: Option<&str>,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        graph::related(&self.conn.lock(), id, rel, as_of, include_invalidated)
    }
    fn graph_invalidate(
        &self,
        _ctx: &WriteContext,
        from_id: &str,
        relation_type: &str,
        to_id: &str,
        ended: Option<&str>,
    ) -> Result<bool, String> {
        graph::invalidate_relation(&self.conn.lock(), from_id, relation_type, to_id, ended)
    }
    fn graph_timeline(
        &self,
        entity_id: &str,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        graph::timeline(&self.conn.lock(), entity_id, include_invalidated)
    }
    fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<graph::TraversalResult, String> {
        graph::traverse(
            &self.conn.lock(),
            start,
            max_hops,
            max_nodes,
            as_of,
            include_invalidated,
        )
    }
    fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
        include_invalidated: bool,
    ) -> Result<Option<Vec<Relation>>, String> {
        graph::shortest_path(&self.conn.lock(), from, to, max_hops, include_invalidated)
    }
    fn graph_tunnels(&self, include_invalidated: bool) -> Result<Vec<serde_json::Value>, String> {
        graph::tunnels(&self.conn.lock(), include_invalidated)
    }
    fn graph_alias(
        &self,
        _ctx: &WriteContext,
        alias_id: &str,
        canonical_id: &str,
    ) -> Result<(), String> {
        graph::add_alias(&self.conn.lock(), alias_id, canonical_id)
    }
    fn graph_resolve(&self, id: &str) -> Result<String, String> {
        graph::resolve_canonical(&self.conn.lock(), id)
    }
    fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String> {
        graph::list_aliases(&self.conn.lock(), canonical_id)
    }
    fn search(&self, q: &SearchQuery, record_access: bool) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        let _read = (!record_access)
            .then(|| crate::manager_search::UntouchedRead::begin(&conn))
            .transpose()?;
        search::run(&conn, q, &self.blobs_dir)
    }
    fn recall(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
        embedder: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
        record_access: bool,
    ) -> Result<Vec<SearchHit>, String> {
        let (vector, conn) = self.query_under_conn(&q.query, embedder);
        let _read = (!record_access)
            .then(|| crate::manager_search::UntouchedRead::begin(&conn))
            .transpose()?;
        search::hybrid_rrf_with_embedding(&conn, q, &self.blobs_dir, cfg, vector.as_ref())
    }
    fn vector_recall(
        &self,
        query: &str,
        k: usize,
        embedder: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
    ) -> Result<Vec<(String, f64)>, String> {
        let (vector, conn) = self.query_under_conn(query, embedder);
        let vector = vector.ok_or_else(|| "query embedding unavailable".to_string())?;
        embeddings::recall_vector(&conn, &vector, k)
    }
    fn vector_stamp(&self) -> Result<Option<EmbedderIdentity>, String> {
        crate::embedder::index::stamp(&self.conn.lock())
    }
    fn init_vectors(&self, _ctx: &WriteContext, identity: &EmbedderIdentity) -> Result<(), String> {
        embeddings::init_index(&self.conn.lock(), identity)
    }
    fn insert_vector(
        &self,
        _ctx: &WriteContext,
        id: &str,
        chunk: usize,
        vector: &StampedVector,
    ) -> Result<crate::embedder::index::InsertOutcome, String> {
        crate::embedder::index::insert_if_live(&self.conn.lock(), id, chunk, vector)
    }
    fn vector_search(
        &self,
        vector: &StampedVector,
        k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        embeddings::recall_vector(&self.conn.lock(), vector, k)
    }
    fn store_vectors(
        &self,
        _ctx: &WriteContext,
        id: &str,
        vectors: Vec<Result<StampedVector, crate::embedder::EmbedError>>,
    ) -> Result<(), String> {
        embeddings::store_vectors(&self.conn.lock(), id, vectors)
    }
    fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String> {
        layers::generate(&self.conn.lock(), opts, &self.blobs_dir)
    }
    fn merge_area(
        &self,
        ctx: &WriteContext,
        from: &str,
        into: &str,
        dry_run: bool,
        dream_run: Option<&str>,
    ) -> Result<store::area_merge::AreaMergeOutcome, String> {
        store::area_merge::merge_area_with_context(
            ctx,
            &self.conn.lock(),
            &self.blobs_dir,
            from,
            into,
            dry_run,
            dream_run,
        )
    }
    fn restore_area(
        &self,
        ctx: &WriteContext,
        id: &str,
        version: Option<i64>,
    ) -> Result<Option<String>, String> {
        store::area_merge::restore_area_with_context(
            ctx,
            &self.conn.lock(),
            &self.blobs_dir,
            id,
            version,
        )
    }
    fn calibration_sweep(
        &self,
        ctx: &WriteContext,
        force: bool,
    ) -> Result<serde_json::Value, String> {
        crate::curation::calibration::run_calibration_sweep_with_context(
            ctx,
            &self.conn.lock(),
            force,
        )
    }
    fn calibration_log(
        &self,
        since: &str,
        limit: usize,
        count_only: bool,
    ) -> Result<serde_json::Value, String> {
        crate::curation::calibration::calibration_log_query(
            &self.conn.lock(),
            since,
            limit,
            count_only,
        )
    }
    fn nightly_calibration(
        &self,
        ctx: &WriteContext,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, String> {
        crate::curation::calibration::run_nightly_calibration_with_context(
            ctx,
            &self.conn.lock(),
            hygiene,
            now,
        )
    }
    fn stale(
        &self,
        ctx: &WriteContext,
        p: &crate::curation::calibration::stale::StaleParams,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
    ) -> Result<serde_json::Value, String> {
        crate::curation::calibration::stale::stale_from_params_with_context(
            ctx,
            &self.conn.lock(),
            p,
            hygiene,
        )
    }
    fn restore_kind(
        &self,
        ctx: &WriteContext,
        id: &str,
        restore: &crate::curation::merge::KindRestore,
        body: Option<&str>,
        actor: &str,
    ) -> Result<serde_json::Value, String> {
        crate::curation::merge::apply_kind_restore_with_context(
            ctx,
            &self.conn.lock(),
            &self.blobs_dir,
            id,
            restore,
            body,
            actor,
        )
    }
    fn merge_items(
        &self,
        ctx: &WriteContext,
        req: &crate::curation::merge::MergeRequest<'_>,
        actor: &str,
        merge_cap: u32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, crate::curation::merge::MergeError> {
        crate::curation::merge::run_merge_with_context(
            ctx,
            &self.conn.lock(),
            &self.blobs_dir,
            req,
            actor,
            merge_cap,
            now,
        )
    }
    fn save(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        replace: bool,
    ) -> Result<crate::manager_crud::SaveOutcome, crate::backend::SaveError> {
        self.save_item(ctx, item, replace)
    }
    fn archive(
        &self,
        ctx: &WriteContext,
        id: &str,
        reason: &str,
        actor: &str,
    ) -> Result<serde_json::Value, crate::curation::merge::MergeError> {
        operations::archive_item(ctx, &self.conn.lock(), id, reason, actor)
    }
    fn is_pinned(&self, id: &str) -> Result<bool, String> {
        self.conn
            .lock()
            .query_row(
                "SELECT is_pinned_wakeup!=0 FROM mem_items WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }
    fn diary_page(&self, offset: usize, limit: usize) -> Result<serde_json::Value, String> {
        operations::diary_page(&self.conn.lock(), &self.blobs_dir, offset, limit)
    }
    fn bump_on_get(&self, _ctx: &WriteContext, kind: &str, id: &str) {
        operations::bump_on_get(&self.conn.lock(), kind, id);
    }
    fn graph_type_counts(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>) {
        operations::graph_type_counts(&self.conn.lock())
    }
    fn access_count(&self, id: &str) -> Result<Option<i64>, String> {
        use rusqlite::OptionalExtension;
        self.conn
            .lock()
            .query_row(
                "SELECT access_count FROM mem_items WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("read access_count: {e}"))
    }
    fn record_access(&self, _ctx: &WriteContext, ids: &[String]) -> Result<(), String> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().to_rfc3339();
        let mut seen = std::collections::HashSet::new();
        for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
            store::touch_access(&conn, id, &now).map_err(|e| format!("record access: {e}"))?;
        }
        Ok(())
    }
    fn graph_candidates(&self, q: &SearchQuery, m: usize) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        let _read = crate::manager_search::UntouchedRead::begin(&conn)?;
        let mut hits = Vec::new();
        for id in search::graph_source::graph_candidates(&conn, q, m)? {
            if let Some(item) = store::get(&conn, &id, &self.blobs_dir)? {
                hits.push(SearchHit { item, score: 0.0 });
            }
        }
        Ok(hits)
    }
}
