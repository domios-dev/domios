use crate::{curation, db_error, graph as pg_graph, items, queries, repairs, PostgresMemStore};
use codeterm_mem::backend::*;
use codeterm_mem::embedder::{EmbedderIdentity, StampedVector};
use codeterm_mem::graph_repair::{
    DuplicateEntityGroup, DuplicateRelationGroup, EntitySimilarityStrategy, MergeReport,
    RelationColumn,
};
use codeterm_mem::store::{GraphDump, HistoryEntry, MemUpdate, PinnedPosition, TailEntry};
use codeterm_mem::{
    graph, search, store, Entity, MemItem, MemStats, NewMemItem, Relation, SearchHit, SearchQuery,
    WakeUpOptions,
};

impl MemStore for PostgresMemStore {
    fn with_export_snapshot(
        &self,
        visitor: &mut dyn FnMut(&dyn codeterm_mem::backend::ExportSnapshot) -> Result<(), String>,
    ) -> Result<(), String> {
        self.snapshot(visitor)
    }

    fn seed_graph(
        &self,
        _ctx: &WriteContext,
        entities: &[(Entity, String)],
        relations: &[Relation],
    ) -> Result<(), String> {
        self.transaction(|tx| {
         for (entity,at) in entities {
          tx.execute("INSERT INTO entities(id,entity_type,scope,properties_json,created_at,updated_at) VALUES ($1,$2,$3,$4,$5,$5) ON CONFLICT DO NOTHING", &[&entity.id,&entity.entity_type,&entity.scope,&entity.properties.to_string(),at]).map_err(db_error)?;
         }
         for edge in relations { pg_graph::insert_in(tx,edge,false)?; }
         Ok(())
        })
    }
    fn deletion_targets(&self, id: &str) -> Result<(Vec<String>, Option<String>), String> {
        self.deletion_targets_in(id)
    }
    fn ids_by_area(&self, area: &str) -> Vec<String> {
        self.transaction(|tx| {
            Ok(tx
                .query(
                    "SELECT id FROM mem_items WHERE area=$1 ORDER BY id",
                    &[&area],
                )
                .map_err(db_error)?
                .into_iter()
                .map(|row| row.get(0))
                .collect())
        })
        .unwrap_or_default()
    }
    fn delete_batch(&self, ctx: &WriteContext, ids: &[String]) -> Result<Deletion, String> {
        self.delete_batch_in(ctx, ids)
    }
    fn recluster_prompt(&self) -> Result<Option<String>, String> {
        self.maintenance_recluster_prompt()
    }
    fn recluster_finalize(
        &self,
        response: &str,
        model: &str,
        plan_dir: &std::path::Path,
    ) -> Result<codeterm_mem::dreaming::recluster::ReclusterReport, String> {
        self.maintenance_recluster_finalize(response, model, plan_dir)
    }
    fn sync_status(&self, timeout: std::time::Duration) -> Option<serde_json::Value> {
        self.sync_status_in(timeout)
    }
    #[cfg(any(test, feature = "test-support"))]
    fn fixture_set_kind(&self, _ctx: &WriteContext, id: &str, kind: &str) -> Result<(), String> {
        self.transaction(|tx| {
            tx.execute("UPDATE mem_items SET kind=$2 WHERE id=$1", &[&id, &kind])
                .map_err(db_error)?;
            Ok(())
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    fn lock_for_test(&self) -> Box<dyn StorageLock + '_> {
        Box::new(self.gate.lock())
    }
    fn export_page(
        &self,
        after: Option<&ExportCursor>,
        limit: usize,
    ) -> Result<ExportPage, String> {
        PostgresMemStore::export_page(self, after, limit)
    }
    fn import_page(&self, _ctx: &WriteContext, page: &ExportPage) -> Result<(), String> {
        PostgresMemStore::import_page(self, page)
    }
    fn table_counts(&self) -> Result<TableCounts, String> {
        PostgresMemStore::table_counts(self)
    }
    fn promote_learning(&self, ctx: &WriteContext, id: &str) -> Result<(), String> {
        self.transaction(|tx| {
            let item = items::get_in(tx, id)?.ok_or("mem item not found")?;
            if item.kind != "learning" {
                return Err(format!(
                    "can only promote kind=learning, got kind={}",
                    item.kind
                ));
            }
            if item.author.is_neighbor() {
                return Err("neighbor content cannot be promoted".into());
            }
            let importance = item.importance.max(4.0);
            tx.execute(
                "UPDATE mem_items SET kind='fact',importance=$2,updated_at=$3 WHERE id=$1",
                &[&id, &importance, &items::now()],
            )
            .map_err(db_error)?;
            items::history_in(
                tx,
                ctx,
                id,
                "promote",
                &store::HistorySnapshot {
                    importance: Some(item.importance),
                    kind: Some("learning".into()),
                    ..Default::default()
                },
                &store::HistorySnapshot {
                    importance: Some(importance),
                    kind: Some("fact".into()),
                    ..Default::default()
                },
                None,
            )
        })
    }
    fn graph_find_duplicate_relations(
        &self,
        group_by: &[RelationColumn],
        limit: usize,
    ) -> Result<Vec<DuplicateRelationGroup>, String> {
        self.transaction(|tx| repairs::duplicate_relations(tx, group_by, limit))
    }
    fn graph_delete_relations(
        &self,
        _ctx: &WriteContext,
        rowids: &[i64],
        actor: &str,
    ) -> Result<u64, String> {
        self.transaction(|tx| repairs::delete_relations(tx, rowids, actor))
    }
    fn graph_delete_entity(
        &self,
        _ctx: &WriteContext,
        entity_id: &str,
        force: bool,
        purge_history: bool,
        actor: &str,
    ) -> Result<codeterm_mem::graph_repair::DeleteEntityReport, String> {
        self.transaction(|tx| repairs::delete_entity(tx, entity_id, force, purge_history, actor))
    }
    fn graph_repair_orphan_vectors(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<codeterm_mem::graph_repair::RepairOrphanVectorsReport, String> {
        self.transaction(|tx| repairs::vector_cleanup(tx, false, dry_run, actor))
    }
    fn graph_prune_thin_vectors(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<codeterm_mem::graph_repair::RepairOrphanVectorsReport, String> {
        self.transaction(|tx| repairs::vector_cleanup(tx, true, dry_run, actor))
    }
    fn graph_collapse_synonym_edges(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<codeterm_mem::graph_repair::RepairOrphanVectorsReport, String> {
        self.transaction(|tx| repairs::edge_cleanup(tx, false, dry_run, actor))
    }
    fn graph_prune_dream_hub_edges(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<codeterm_mem::graph_repair::RepairOrphanVectorsReport, String> {
        self.transaction(|tx| repairs::edge_cleanup(tx, true, dry_run, actor))
    }
    fn graph_repair_orphan_blobs(
        &self,
        _ctx: &WriteContext,
        dry_run: bool,
        actor: &str,
    ) -> Result<codeterm_mem::graph_repair::RepairOrphanBlobsReport, String> {
        self.transaction(|tx| repairs::blob_cleanup(tx, dry_run, actor))
    }
    fn graph_merge_relations(
        &self,
        _ctx: &WriteContext,
        keep: i64,
        merge: &[i64],
        actor: &str,
    ) -> Result<MergeReport, String> {
        self.transaction(|tx| repairs::merge_relations(tx, keep, merge, actor))
    }
    fn graph_find_duplicate_entities(
        &self,
        strategy: EntitySimilarityStrategy,
        limit: usize,
    ) -> Result<Vec<DuplicateEntityGroup>, String> {
        self.transaction(|tx| repairs::duplicate_entities(tx, strategy, limit))
    }
    fn graph_augment_entity(
        &self,
        _ctx: &WriteContext,
        entity_id: &str,
        patch: serde_json::Map<String, serde_json::Value>,
        actor: &str,
    ) -> Result<(), String> {
        self.transaction(|tx| repairs::augment_entity(tx, entity_id, patch, actor))
    }
    fn entity_embed_text(&self, id: &str) -> Option<String> {
        self.transaction(|tx| {
            Ok(tx
                .query_opt(
                    "SELECT entity_type,properties_json FROM entities WHERE id=$1",
                    &[&id],
                )
                .map_err(db_error)?
                .map(|row| {
                    codeterm_mem::embeddings::embeddable_text_for_entity(
                        id,
                        &row.get::<_, String>(0),
                        &row.get::<_, String>(1),
                    )
                }))
        })
        .ok()
        .flatten()
    }
    fn insert(&self, ctx: &WriteContext, item: &NewMemItem) -> Result<String, String> {
        PostgresMemStore::insert(self, ctx, item)
    }
    fn insert_seeded(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        fingerprint: &str,
    ) -> Result<String, String> {
        self.transaction(|tx| items::insert_in(tx, ctx, item, Some(fingerprint)))
    }
    fn get(&self, id: &str) -> Result<Option<MemItem>, String> {
        PostgresMemStore::get(self, id)
    }
    fn update(&self, ctx: &WriteContext, id: &str, patch: &MemUpdate) -> Result<bool, String> {
        PostgresMemStore::update(self, ctx, id, patch)
    }
    fn delete(&self, ctx: &WriteContext, id: &str) -> Result<bool, String> {
        PostgresMemStore::delete(self, ctx, id)
    }
    fn managed_payload(&self, id: &str) -> Result<Option<(NewMemItem, Option<String>)>, String> {
        self.transaction(|tx| items::payload_in(tx, id))
    }
    fn cas_refresh_bundled(
        &self,
        ctx: &WriteContext,
        id: &str,
        expected_fingerprint: Option<&str>,
        new_item: &NewMemItem,
        new_fingerprint: &str,
    ) -> Result<bool, String> {
        self.cas_refresh_in(ctx, id, expected_fingerprint, new_item, new_fingerprint)
    }
    fn bootstrap_seed_fingerprint(
        &self,
        _ctx: &WriteContext,
        id: &str,
        expected: &NewMemItem,
        fingerprint: &str,
    ) -> Result<bool, String> {
        self.transaction(|tx| {
            let Some((current, stamp)) = items::payload_in(tx, id)? else {
                return Ok(false);
            };
            if stamp.is_some() || current != *expected {
                return Ok(false);
            }
            tx.execute(
                "UPDATE mem_items SET seed_fingerprint=$2 WHERE id=$1 AND seed_fingerprint IS NULL",
                &[&id, &fingerprint],
            )
            .map_err(db_error)?;
            Ok(true)
        })
    }
    fn graph_full(&self) -> Result<GraphDump, String> {
        self.transaction(queries::graph_dump_in)
    }
    fn list_pinned_positions(&self) -> Result<Vec<PinnedPosition>, String> {
        self.transaction(|tx| {
            Ok(tx
                .query(
                    "SELECT mem_id,x,y FROM graph_pinned_positions ORDER BY updated_at DESC",
                    &[],
                )
                .map_err(db_error)?
                .into_iter()
                .map(|row| PinnedPosition {
                    mem_id: row.get(0),
                    x: row.get(1),
                    y: row.get(2),
                })
                .collect())
        })
    }
    fn list_pinned(&self, limit: usize) -> Result<Vec<MemItem>, String> {
        self.transaction(|tx|queries::items_in(tx,"SELECT id FROM mem_items WHERE is_pinned_wakeup=1 AND kind!='archived' ORDER BY importance DESC,evidence_count DESC,updated_at DESC LIMIT $1", &[&crate::pg_limit(limit.clamp(1,500))]))
    }
    fn list_by_source_prefix(&self, kind: &str, prefix: &str) -> Result<Vec<MemItem>, String> {
        self.transaction(|tx|queries::items_in(tx,"SELECT id FROM mem_items WHERE kind=$1 AND source IS NOT NULL AND left(source,length($2))=$2 ORDER BY id", &[&kind,&prefix]))
    }
    fn set_pinned_position(
        &self,
        _ctx: &WriteContext,
        mem_id: &str,
        x: f64,
        y: f64,
    ) -> Result<bool, String> {
        self.transaction(|tx| {
         if items::get_in(tx,mem_id)?.is_none() {return Ok(false);}
         tx.execute("INSERT INTO graph_pinned_positions(mem_id,x,y,updated_at) VALUES ($1,$2,$3,$4) ON CONFLICT(mem_id) DO UPDATE SET x=excluded.x,y=excluded.y,updated_at=excluded.updated_at", &[&mem_id,&x,&y,&chrono::Utc::now().timestamp()]).map_err(db_error)?; Ok(true)
        })
    }
    fn delete_pinned_position(&self, _ctx: &WriteContext, mem_id: &str) -> Result<usize, String> {
        self.transaction(|tx| {
            tx.execute(
                "DELETE FROM graph_pinned_positions WHERE mem_id=$1",
                &[&mem_id],
            )
            .map(|count| count as usize)
            .map_err(db_error)
        })
    }
    fn stats(&self) -> Result<MemStats, String> {
        self.transaction(queries::stats_in)
    }
    fn get_item_history(&self, id: &str) -> Result<Vec<HistoryEntry>, String> {
        PostgresMemStore::get_item_history(self, id)
    }
    fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, u64>>, String>
    {
        self.transaction(|tx| queries::taxonomy_in(tx, workspace))
    }
    fn tail_recent(&self, since: Option<&str>, limit: usize) -> Result<Vec<TailEntry>, String> {
        self.transaction(|tx| queries::tail_in(tx, since, limit))
    }
    fn dream_diaries_since(&self, since_ms: u64) -> Result<Vec<String>, String> {
        self.transaction(|tx| {
         let since=chrono::DateTime::<chrono::Utc>::from_timestamp_millis(i64::try_from(since_ms).map_err(|_|"diary cutoff overflow")?).ok_or("invalid diary cutoff")?.to_rfc3339();
         Ok(queries::items_in(tx,"SELECT id FROM mem_items WHERE kind='daily' AND area='_dreams' AND created_at>$1 ORDER BY created_at", &[&since])?.into_iter().map(|item|item.body).collect())
        })
    }
    fn graph_create_entity(&self, _ctx: &WriteContext, e: &Entity) -> Result<(), String> {
        self.transaction(|tx| pg_graph::entity_in(tx, e))
    }
    fn graph_relate(&self, _ctx: &WriteContext, r: &Relation) -> Result<(), String> {
        self.transaction(|tx| pg_graph::insert_in(tx, r, true))
    }
    fn graph_insert_relation(&self, _ctx: &WriteContext, r: &Relation) -> Result<(), String> {
        self.transaction(|tx| pg_graph::insert_in(tx, r, false))
    }
    fn graph_related(
        &self,
        id: &str,
        rel: Option<&str>,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        self.transaction(|tx| pg_graph::related_in(tx, id, rel, as_of, include_invalidated, false))
    }
    fn graph_invalidate(
        &self,
        _ctx: &WriteContext,
        from_id: &str,
        relation_type: &str,
        to_id: &str,
        ended: Option<&str>,
    ) -> Result<bool, String> {
        self.transaction(|tx|tx.execute("UPDATE relations SET valid_to=$4 WHERE from_id=$1 AND relation_type=$2 AND to_id=$3 AND valid_to IS NULL", &[&from_id,&relation_type,&to_id,&ended.map(str::to_owned).unwrap_or_else(items::now)]).map(|count|count>0).map_err(db_error))
    }
    fn graph_timeline(
        &self,
        entity_id: &str,
        include_invalidated: bool,
    ) -> Result<Vec<Relation>, String> {
        self.transaction(|tx| {
            pg_graph::related_in(tx, entity_id, None, None, include_invalidated, true)
        })
    }
    fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
        include_invalidated: bool,
    ) -> Result<graph::TraversalResult, String> {
        self.transaction(|tx| {
            pg_graph::traverse_in(tx, start, max_hops, max_nodes, as_of, include_invalidated)
        })
    }
    fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
        include_invalidated: bool,
    ) -> Result<Option<Vec<Relation>>, String> {
        self.transaction(|tx| pg_graph::path_in(tx, from, to, max_hops, include_invalidated))
    }
    fn graph_tunnels(&self, include_invalidated: bool) -> Result<Vec<serde_json::Value>, String> {
        self.tunnels_in(include_invalidated)
    }
    fn graph_alias(
        &self,
        _ctx: &WriteContext,
        alias_id: &str,
        canonical_id: &str,
    ) -> Result<(), String> {
        self.transaction(|tx| pg_graph::alias_in(tx, alias_id, canonical_id))
    }
    fn graph_resolve(&self, id: &str) -> Result<String, String> {
        self.transaction(|tx| {
            Ok(tx
                .query_opt(
                    "SELECT canonical_id FROM entity_aliases WHERE alias_id=$1",
                    &[&id],
                )
                .map_err(db_error)?
                .map(|row| row.get(0))
                .unwrap_or_else(|| id.to_owned()))
        })
    }
    fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String> {
        self.transaction(|tx| {
            Ok(tx
                .query(
                    "SELECT alias_id FROM entity_aliases WHERE canonical_id=$1 ORDER BY alias_id",
                    &[&canonical_id],
                )
                .map_err(db_error)?
                .into_iter()
                .map(|row| row.get(0))
                .collect())
        })
    }
    fn search(&self, q: &SearchQuery, record_access: bool) -> Result<Vec<SearchHit>, String> {
        self.transaction(|tx| {
            let hits = queries::search_in(tx, q, &search::SearchConfig::default())?;
            if record_access {
                queries::touch_in(
                    tx,
                    &hits
                        .iter()
                        .map(|hit| hit.item.id.clone())
                        .collect::<Vec<_>>(),
                )?;
            }
            Ok(hits)
        })
    }
    fn recall(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
        embedder: &std::sync::Arc<codeterm_mem::embedder::ActiveEmbedder>,
        record_access: bool,
    ) -> Result<Vec<SearchHit>, String> {
        self.recall_in(q, cfg, embedder, record_access)
    }
    fn vector_recall(
        &self,
        query: &str,
        k: usize,
        embedder: &std::sync::Arc<codeterm_mem::embedder::ActiveEmbedder>,
    ) -> Result<Vec<(String, f64)>, String> {
        let vector = embedder.query(query).ok_or("query embedding unavailable")?;
        PostgresMemStore::vector_search(self, &vector, k)
    }
    fn vector_stamp(&self) -> Result<Option<EmbedderIdentity>, String> {
        PostgresMemStore::vector_stamp(self)
    }
    fn init_vectors(&self, _ctx: &WriteContext, identity: &EmbedderIdentity) -> Result<(), String> {
        PostgresMemStore::init_vectors(self, identity)
    }
    fn insert_vector(
        &self,
        _ctx: &WriteContext,
        id: &str,
        chunk: usize,
        vector: &StampedVector,
    ) -> Result<codeterm_mem::embedder::index::InsertOutcome, String> {
        PostgresMemStore::insert_vector(self, id, chunk, vector)
    }
    fn vector_search(
        &self,
        vector: &StampedVector,
        k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        PostgresMemStore::vector_search(self, vector, k)
    }
    fn store_vectors(
        &self,
        _ctx: &WriteContext,
        id: &str,
        vectors: Vec<Result<StampedVector, codeterm_mem::embedder::EmbedError>>,
    ) -> Result<(), String> {
        self.store_vectors_in(id, vectors)
    }
    fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String> {
        self.transaction(|tx| crate::wake_up::generate_in(tx, opts))
    }
    fn merge_area(
        &self,
        ctx: &WriteContext,
        from: &str,
        into: &str,
        dry_run: bool,
        dream_run: Option<&str>,
    ) -> Result<store::area_merge::AreaMergeOutcome, String> {
        self.transaction(|tx| curation::merge_area_in(tx, ctx, from, into, dry_run, dream_run))
    }
    fn restore_area(
        &self,
        ctx: &WriteContext,
        id: &str,
        version: Option<i64>,
    ) -> Result<Option<String>, String> {
        self.transaction(|tx| curation::restore_area_in(tx, ctx, id, version))
    }
    fn calibration_sweep(
        &self,
        ctx: &WriteContext,
        force: bool,
    ) -> Result<serde_json::Value, String> {
        self.maintenance_calibration_sweep(ctx, force)
    }
    fn calibration_log(
        &self,
        since: &str,
        limit: usize,
        count_only: bool,
    ) -> Result<serde_json::Value, String> {
        self.maintenance_calibration_log(since, limit, count_only)
    }
    fn nightly_calibration(
        &self,
        ctx: &WriteContext,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, String> {
        self.maintenance_nightly_calibration(ctx, hygiene, now)
    }
    fn stale(
        &self,
        ctx: &WriteContext,
        p: &codeterm_mem::curation::calibration::stale::StaleParams,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
    ) -> Result<serde_json::Value, String> {
        self.maintenance_stale(ctx, p, hygiene)
    }
    fn restore_kind(
        &self,
        ctx: &WriteContext,
        id: &str,
        restore: &codeterm_mem::curation::merge::KindRestore,
        body: Option<&str>,
        actor: &str,
    ) -> Result<serde_json::Value, String> {
        self.restore_kind_in(ctx, id, restore, body, actor)
    }
    fn merge_items(
        &self,
        ctx: &WriteContext,
        req: &codeterm_mem::curation::merge::MergeRequest<'_>,
        actor: &str,
        merge_cap: u32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, codeterm_mem::curation::merge::MergeError> {
        self.merge_items_in(ctx, req, actor, merge_cap, now)
    }
    fn save(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        replace: bool,
    ) -> Result<codeterm_mem::manager_crud::SaveOutcome, SaveError> {
        PostgresMemStore::save(self, ctx, item, replace)
    }
    fn archive(
        &self,
        ctx: &WriteContext,
        id: &str,
        reason: &str,
        actor: &str,
    ) -> Result<serde_json::Value, codeterm_mem::curation::merge::MergeError> {
        self.archive_item(ctx, id, reason, actor)
    }
    fn is_pinned(&self, id: &str) -> Result<bool, String> {
        self.transaction(|tx| {
            Ok(tx
                .query_one(
                    "SELECT is_pinned_wakeup!=0 FROM mem_items WHERE id=$1",
                    &[&id],
                )
                .map_err(db_error)?
                .get(0))
        })
    }
    fn diary_page(&self, offset: usize, limit: usize) -> Result<serde_json::Value, String> {
        self.diary_in(offset, limit)
    }
    fn bump_on_get(&self, _ctx: &WriteContext, kind: &str, id: &str) {
        if matches!(kind, "skill" | "lesson" | "rule" | "fact" | "preference") {
            let _ = self.transaction(|tx| {
                tx.execute(
                    "UPDATE mem_items SET evidence_count=evidence_count+1 WHERE id=$1",
                    &[&id],
                )
                .map(|_| ())
                .map_err(db_error)
            });
        }
    }
    fn graph_type_counts(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>) {
        self.type_counts_in()
    }
    fn access_count(&self, id: &str) -> Result<Option<i64>, String> {
        self.transaction(|tx| {
            Ok(tx
                .query_opt("SELECT access_count FROM mem_items WHERE id=$1", &[&id])
                .map_err(db_error)?
                .map(|row| row.get(0)))
        })
    }
    fn record_access(&self, _ctx: &WriteContext, ids: &[String]) -> Result<(), String> {
        self.transaction(|tx| queries::touch_in(tx, ids))
    }
    fn graph_candidates(&self, q: &SearchQuery, m: usize) -> Result<Vec<SearchHit>, String> {
        self.graph_candidates_in(q, m)
    }
}
