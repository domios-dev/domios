use super::SqliteMemStore;
use crate::backend::{engines::*, MemStore, WriteContext};
use crate::embedder::{index, pending, ActiveEmbedder, EmbedderIdentity};
use crate::graph_layout::{cache::LayoutCache, project, snapshot};
use std::sync::Arc;
mod dreaming;
mod extraction;
pub(crate) mod missing;
pub(crate) mod purge;
mod rem;
mod shadow;
mod sources;
pub(crate) mod targets;
pub mod worker;

impl SqliteMemStore {
    fn engine_lock(
        &self,
        wait: bool,
    ) -> Result<parking_lot::MutexGuard<'_, rusqlite::Connection>, String> {
        if wait {
            Ok(self.conn.lock())
        } else {
            self.conn
                .try_lock()
                .ok_or_else(|| "memory is busy; embedder switch cannot complete now".into())
        }
    }
    fn plan_in(
        &self,
        conn: &rusqlite::Connection,
        docs: u64,
    ) -> Result<crate::embedder::SwitchPlan, String> {
        Ok(crate::embedder::SwitchPlan {
            blueprints: crate::backend::sqlite::blueprint::index::indexed_items(conn)?.len() as u64,
            items: self.item_targets_in(conn)?.len() as u64,
            entities: self.entity_targets_in(conn)?.len() as u64,
            docs,
        })
    }
}

impl EngineStore for SqliteMemStore {
    fn maintenance_mode(&self) -> MaintenanceMode {
        let file = self
            .conn
            .lock()
            .query_row(
                "SELECT file FROM pragma_database_list WHERE name='main'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default();
        if file.is_empty() {
            MaintenanceMode::InProcess
        } else {
            MaintenanceMode::Subprocess {
                db_path: file.into(),
            }
        }
    }
    fn background_store(&self) -> Result<Arc<dyn MemStore>, String> {
        Ok(Arc::new(Self::from_connection(
            self.conn.clone(),
            self.blobs_dir.clone(),
        )))
    }
    fn engine_status(&self, wait: bool) -> Result<EngineStatus, String> {
        let conn = self.engine_lock(wait)?;
        Ok((
            index::stamp(&conn)?,
            index::vector_count(&conn),
            pending::count(&conn),
        ))
    }
    fn engine_plan(&self, docs: u64) -> Result<crate::embedder::SwitchPlan, String> {
        self.plan_in(&self.conn.lock(), docs)
    }
    fn prepare_engine_switch(
        &self,
        _ctx: &WriteContext,
        identity: &EmbedderIdentity,
        docs: u64,
        wait: bool,
    ) -> Result<crate::embedder::SwitchPlan, String> {
        let conn = self.engine_lock(wait)?;
        let plan = self.plan_in(&conn, docs)?;
        self.prepare_shadow_in(&conn, identity)?;
        Ok(plan)
    }
    fn clear_shadow(&self, _ctx: &WriteContext, wait: bool) -> Result<(), String> {
        let conn = self.engine_lock(wait)?;
        self.clear_shadow_in(&conn);
        Ok(())
    }
    fn adopt_published(
        &self,
        _ctx: &WriteContext,
        active: &ActiveEmbedder,
        provider: crate::embedder::provider::SharedProvider,
    ) {
        let _guard = self.conn.lock();
        active.replace_silent(provider);
    }
    fn reembed_item_targets(&self) -> Result<Vec<(String, String)>, String> {
        self.item_targets_in(&self.conn.lock())
    }
    fn reembed_entity_targets(&self) -> Result<Vec<(String, String)>, String> {
        self.entity_targets_in(&self.conn.lock())
    }
    fn blueprint_reembed_targets(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.conn.lock();
        crate::backend::sqlite::blueprint::index::indexed_items(&conn)?
            .into_iter()
            .map(|id| {
                let texts = crate::backend::sqlite::blueprint::vector_projection(&conn, &id)?
                    .map(|(row, intents)| {
                        crate::blueprint_library::enrichment::blueprint_vector_texts(&row, &intents)
                    })
                    .unwrap_or_default();
                Ok((
                    id,
                    texts
                        .into_iter()
                        .map(|(_, _, text)| text)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ))
            })
            .collect()
    }
    fn missing_item_vectors(&self) -> Vec<(String, String)> {
        missing::query_missing_vectors(&self.conn.lock(), &self.blobs_dir)
    }
    fn missing_entity_vectors(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT e.id,e.entity_type,e.properties_json FROM entities e LEFT JOIN mem_vectors v ON v.mem_id=e.id WHERE v.mem_id IS NULL").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            let (id, kind, props) = r.map_err(|e| e.to_string())?;
            let text = crate::embeddings::embeddable_text_for_entity(&id, &kind, &props);
            Ok((id, text))
        })
        .collect()
    }
    fn pending_ids(&self) -> Result<Vec<String>, String> {
        pending::ids(&self.conn.lock())
    }
    fn mark_pending(&self, _ctx: &WriteContext, id: &str, reason: &str) -> Result<(), String> {
        pending::mark(&self.conn.lock(), id, reason)
    }
    fn clear_pending(&self, _ctx: &WriteContext, id: &str) -> Result<(), String> {
        pending::clear(&self.conn.lock(), id)
    }
    fn pending_embed_text(&self, id: &str) -> Option<String> {
        self.pending_text_in(id)
    }
    fn shadow_stale(&self, stamp: &str) -> Result<(Vec<ShadowUnit>, u64), String> {
        let conn = self.conn.lock();
        let units = self.shadow_units(&conn)?;
        let total = units.len() as u64;
        let mut stale = Vec::new();
        for unit in units {
            if shadow::checkpoint(&conn, stamp, &unit.key())?
                .is_none_or(|(hash, _)| hash != unit.hash())
            {
                stale.push(unit);
            }
        }
        Ok((stale, total))
    }
    fn shadow_checkpoint(
        &self,
        _ctx: &WriteContext,
        stamp: &str,
        unit: &ShadowUnit,
        vectors: Option<&[u8]>,
        recovery: bool,
        current: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        if !current() {
            return Err("re-embed cancelled".into());
        }
        if recovery {
            conn.execute("UPDATE reembed.shadow SET vectors=?3 WHERE target=?1 AND unit_key=?2", rusqlite::params![stamp,unit.key(),vectors])
        } else {
            conn.execute("INSERT OR REPLACE INTO reembed.shadow(target,unit_key,text_hash,vectors) VALUES(?1,?2,?3,?4)", rusqlite::params![stamp,unit.key(),unit.hash(),vectors])
        }.map_err(|e| e.to_string())?;
        Ok(())
    }
    fn publish_shadow(
        &self,
        _ctx: &WriteContext,
        target: &ActiveEmbedder,
        active: &ActiveEmbedder,
        stamp: &str,
        claim: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        self.publish_shadow_in(target, active, stamp, claim)
    }
    fn checkpoint(&self) -> Result<(i64, i64, i64), String> {
        self.conn
            .lock()
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|e| format!("wal_checkpoint(TRUNCATE) failed: {e}"))
    }
    fn purge_precompact(
        &self,
        ctx: &WriteContext,
        id: &str,
        apply: bool,
        verified_deleted: bool,
        vacuum: bool,
    ) -> Result<crate::precompact_purge::PurgeReport, String> {
        self.purge_precompact_in(ctx, id, apply, verified_deleted, vacuum)
    }
    fn tracked_sources(&self) -> Result<Vec<TrackedSource>, String> {
        self.tracked_sources_in()
    }
    fn refresh_source(
        &self,
        ctx: &WriteContext,
        expected: &TrackedSource,
        refresh: &SourceRefresh<'_>,
    ) -> Result<bool, String> {
        self.refresh_source_in(ctx, expected, refresh)
    }
    fn pending_extraction(&self, limit: usize) -> Result<Vec<PendingExtraction>, String> {
        self.pending_extraction_in(limit)
    }
    fn bump_evidence(&self, _ctx: &WriteContext, id: &str) -> Result<bool, String> {
        codeterm_mem_store::graph::bump_evidence_count(&self.conn.lock(), id)
            .map_err(|e| e.to_string())
    }
    fn dream_candidates(&self, before: &str) -> Result<Vec<LightCandidate>, String> {
        codeterm_mem_store::maintenance::light_candidates(&self.conn.lock(), before)
    }
    fn dream_promote(
        &self,
        ctx: &WriteContext,
        candidate: &LightCandidate,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String> {
        {
            let conn = self.conn.lock();
            crate::store::refuse_neighbor_promotion(&conn, &candidate.0)?;
            let author = ctx.default_system_author("dreaming:rem");
            let audit = codeterm_mem_store::maintenance::PromotionAudit {
                changed_by: &ctx.changed_by,
                kind: author.kind.as_str(),
                label: author.label.as_deref(),
                uid: author.account_uid.as_deref(),
                friendship: author.friendship_id.as_deref(),
            };
            codeterm_mem_store::maintenance::promote_candidate(&conn, candidate, now, &audit)
        }
    }
    fn dream_expire(&self, _ctx: &WriteContext, kind: &str, days: i64) -> Result<u64, String> {
        codeterm_mem_store::maintenance::expire_tier(&self.conn.lock(), kind, days)
    }
    fn dream_normalize(&self, _ctx: &WriteContext) -> Result<serde_json::Value, String> {
        worker::worker_dreaming_normalize(&self.conn.lock())
    }
    fn dream_worker(
        &self,
        ctx: &WriteContext,
        kind: &str,
        machine: &str,
        _timeout: std::time::Duration,
    ) -> Result<serde_json::Value, String> {
        self.dream_worker_in(ctx, kind, machine)
    }
    fn rem_snapshot(&self, blueprint_merges: usize) -> Result<String, String> {
        self.rem_snapshot_in(blueprint_merges)
    }
    fn sync_skill_folders(
        &self,
        _ctx: &WriteContext,
        root: &std::path::Path,
    ) -> codeterm_mem_store::skill_folders::SkillFolderReport {
        codeterm_mem_store::skill_folders::sync_skill_folders(&self.conn.lock(), root)
    }
    fn layout_run_full(&self, cache: &LayoutCache) -> Result<u64, String> {
        project::run_full(&self.conn.lock(), cache)
    }
    fn layout_snapshot(&self, cache: &LayoutCache) -> Result<snapshot::Snapshot, String> {
        snapshot::build_snapshot(&self.conn.lock(), cache)
    }
    fn layout_delta(
        &self,
        cache: &LayoutCache,
        since: u64,
    ) -> Result<snapshot::DeltaResponse, String> {
        snapshot::build_delta(&self.conn.lock(), cache, since)
    }
    fn layout_labels(&self, ids: &[String]) -> Result<Vec<(String, String)>, String> {
        snapshot::load_labels(&self.conn.lock(), ids)
    }
    fn layout_edges(&self, ids: &[String]) -> Result<Vec<snapshot::EdgeRow>, String> {
        snapshot::load_edges(&self.conn.lock(), ids)
    }
    fn layout_vectors(&self) -> Result<Vec<(String, Vec<f32>)>, String> {
        project::load_primary_vectors(&self.conn.lock())
    }
    #[cfg(feature = "embeddings")]
    fn layout_place(&self, cache: &LayoutCache, id: &str, vector: &[f32]) -> Result<bool, String> {
        crate::graph_layout::placement::place_incremental(&self.conn.lock(), cache, id, vector)
    }
    #[cfg(not(feature = "embeddings"))]
    fn layout_place(
        &self,
        _cache: &LayoutCache,
        _id: &str,
        _vector: &[f32],
    ) -> Result<bool, String> {
        Ok(false)
    }
    #[cfg(feature = "embeddings")]
    fn layout_place_stored(&self, cache: &LayoutCache, id: &str) {
        if cache.is_empty() {
            return;
        }
        let conn = self.conn.lock();
        let blob = conn
            .query_row(
                "SELECT embedding FROM mem_vectors WHERE mem_id=?1",
                [id],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .ok();
        if let Some(blob) = blob {
            let vector = crate::graph_layout::placement::decode_vector(&blob);
            if let Err(error) =
                crate::graph_layout::placement::place_incremental(&conn, cache, id, &vector)
            {
                log::warn!("[graph_layout] incremental place {id}: {error}");
            }
        }
    }
    #[cfg(not(feature = "embeddings"))]
    fn layout_place_stored(&self, _cache: &LayoutCache, _id: &str) {}
    #[cfg(any(test, feature = "test-support"))]
    fn fixture_backend(&self) -> &dyn std::any::Any {
        self
    }
}
