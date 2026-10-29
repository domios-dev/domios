use crate::{db_error, vectors, PostgresMemStore};
use postgres::Transaction;
mod dreaming;
mod layout;
mod purge;
mod rem;
mod shadow;
mod skills;
mod sources;
mod targets;
use codeterm_mem::backend::{engines::*, MemStore, WriteContext};
use codeterm_mem::embedder::{ActiveEmbedder, EmbedderIdentity};
use codeterm_mem::graph_layout::{cache::LayoutCache, snapshot};
use std::{sync::Arc, time::Duration};

impl EngineStore for PostgresMemStore {
    fn maintenance_mode(&self) -> MaintenanceMode {
        MaintenanceMode::InProcess
    }
    fn background_store(&self) -> Result<Arc<dyn MemStore>, String> {
        Ok(Arc::new(self.clone()))
    }
    fn engine_status(&self, wait: bool) -> Result<EngineStatus, String> {
        self.engine_transaction(wait, |tx| {
            Ok((
                vectors::stamp_in(tx)?,
                shadow::vector_count(tx)?,
                count(tx, "mem_vector_pending")?,
            ))
        })
    }
    fn engine_plan(&self, docs: u64) -> Result<codeterm_mem::embedder::switch::SwitchPlan, String> {
        self.transaction(|tx| targets::plan(tx, docs))
    }
    fn prepare_engine_switch(
        &self,
        _ctx: &WriteContext,
        identity: &EmbedderIdentity,
        docs: u64,
        wait: bool,
    ) -> Result<codeterm_mem::embedder::switch::SwitchPlan, String> {
        self.engine_transaction(wait, |tx| {
            let plan = targets::plan(tx, docs)?;
            let stamp = serde_json::to_string(identity).map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM mem_shadow WHERE target<>$1 OR vectors IS NULL",
                &[&stamp],
            )
            .map_err(db_error)?;
            Ok(plan)
        })
    }
    fn clear_shadow(&self, _ctx: &WriteContext, wait: bool) -> Result<(), String> {
        self.engine_transaction(wait, |tx| {
            tx.execute("DELETE FROM mem_shadow", &[])
                .map_err(db_error)?;
            Ok(())
        })
    }
    fn adopt_published(
        &self,
        _ctx: &WriteContext,
        active: &ActiveEmbedder,
        provider: codeterm_mem::embedder::provider::SharedProvider,
    ) {
        let _gate = self.gate.lock();
        if self
            .transaction_locked(true, |_| Ok::<_, String>(()))
            .is_ok()
        {
            active.replace_silent(provider);
        }
    }
    fn reembed_item_targets(&self) -> Result<Vec<(String, String)>, String> {
        self.transaction(|tx| targets::items(tx, false))
    }
    fn reembed_entity_targets(&self) -> Result<Vec<(String, String)>, String> {
        self.transaction(|tx| targets::entities(tx, false))
    }
    fn blueprint_reembed_targets(&self) -> Result<Vec<(String, String)>, String> {
        self.transaction(|tx| {
            Ok(targets::blueprints(tx)?
                .into_iter()
                .map(|u| {
                    (
                        u.id,
                        u.texts
                            .into_iter()
                            .map(|(_, _, t)| t)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                })
                .collect())
        })
    }
    fn missing_item_vectors(&self) -> Vec<(String, String)> {
        self.transaction(|tx| targets::items(tx, true))
            .unwrap_or_default()
    }
    fn missing_entity_vectors(&self) -> Result<Vec<(String, String)>, String> {
        self.transaction(|tx| targets::entities(tx, true))
    }
    fn pending_ids(&self) -> Result<Vec<String>, String> {
        self.transaction(|tx| {
            Ok(tx
                .query(
                    "SELECT mem_id FROM mem_vector_pending ORDER BY queued_at,mem_id",
                    &[],
                )
                .map_err(db_error)?
                .into_iter()
                .map(|r| r.get(0))
                .collect())
        })
    }
    fn mark_pending(&self, _ctx: &WriteContext, id: &str, reason: &str) -> Result<(), String> {
        self.transaction(|tx| mark(tx, id, reason))
    }
    fn clear_pending(&self, _ctx: &WriteContext, id: &str) -> Result<(), String> {
        self.transaction(|tx| {
            tx.execute("DELETE FROM mem_vector_pending WHERE mem_id=$1", &[&id])
                .map_err(db_error)?;
            Ok(())
        })
    }
    fn pending_embed_text(&self, id: &str) -> Option<String> {
        self.transaction(|tx| targets::pending_text(tx, id))
            .ok()
            .flatten()
    }
    fn shadow_stale(&self, stamp: &str) -> Result<(Vec<ShadowUnit>, u64), String> {
        self.transaction(|tx| {
            let units = targets::units(tx)?;
            let total = units.len() as u64;
            let mut stale = Vec::new();
            for unit in units {
                if shadow::checkpoint(tx, stamp, &unit.key())?
                    .is_none_or(|(hash, _)| hash != unit.hash())
                {
                    stale.push(unit);
                }
            }
            Ok((stale, total))
        })
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
        self.transaction(|tx| { if !current() { return Err("re-embed cancelled".into()); } if recovery { tx.execute("UPDATE mem_shadow SET vectors=$3 WHERE target=$1 AND unit_key=$2", &[&stamp,&unit.key(),&vectors]).map_err(db_error)?; } else { tx.execute("INSERT INTO mem_shadow(target,unit_key,text_hash,vectors) VALUES ($1,$2,$3,$4) ON CONFLICT(target,unit_key) DO UPDATE SET text_hash=excluded.text_hash,vectors=excluded.vectors", &[&stamp,&unit.key(),&unit.hash(),&vectors]).map_err(db_error)?; } Ok(()) })
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
        // Postgres owns WAL checkpoints; the client has no local WAL to truncate.
        Ok((0, 0, 0))
    }
    fn purge_precompact(
        &self,
        _ctx: &WriteContext,
        id: &str,
        apply: bool,
        verified_deleted: bool,
        vacuum: bool,
    ) -> Result<codeterm_mem::precompact_purge::PurgeReport, String> {
        self.purge_precompact_in(id, apply, verified_deleted, vacuum)
    }
    fn tracked_sources(&self) -> Result<Vec<TrackedSource>, String> {
        self.transaction(sources::tracked)
    }
    fn refresh_source(
        &self,
        ctx: &WriteContext,
        expected: &TrackedSource,
        refresh: &SourceRefresh<'_>,
    ) -> Result<bool, String> {
        self.transaction(|tx| sources::refresh(tx, ctx, expected, refresh))
    }
    fn pending_extraction(&self, limit: usize) -> Result<Vec<PendingExtraction>, String> {
        self.transaction(|tx| sources::extraction(tx, limit))
    }
    fn bump_evidence(&self, _ctx: &WriteContext, id: &str) -> Result<bool, String> {
        self.transaction(|tx| {
            Ok(tx
                .execute(
                    "UPDATE mem_items SET evidence_count=evidence_count+1 WHERE id=$1",
                    &[&id],
                )
                .map_err(db_error)?
                > 0)
        })
    }
    fn dream_candidates(&self, before: &str) -> Result<Vec<LightCandidate>, String> {
        self.transaction(|tx| dreaming::candidates(tx, before))
    }
    fn dream_promote(
        &self,
        ctx: &WriteContext,
        candidate: &LightCandidate,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String> {
        self.transaction(|tx| dreaming::promote(tx, ctx, candidate, now, true))
    }
    fn dream_expire(&self, _ctx: &WriteContext, kind: &str, days: i64) -> Result<u64, String> {
        self.transaction(|tx| dreaming::expire(tx, kind, days))
    }
    fn dream_normalize(&self, _ctx: &WriteContext) -> Result<serde_json::Value, String> {
        self.transaction(dreaming::normalize)
    }
    fn dream_worker(
        &self,
        ctx: &WriteContext,
        kind: &str,
        machine: &str,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        self.transaction(|tx| dreaming::worker(tx, ctx, kind, machine, timeout))
    }
    fn rem_snapshot(&self, blueprint_merges: usize) -> Result<String, String> {
        self.transaction(|tx| rem::snapshot(tx, blueprint_merges))
    }
    fn sync_skill_folders(
        &self,
        _ctx: &WriteContext,
        root: &std::path::Path,
    ) -> codeterm_mem_store::skill_folders::SkillFolderReport {
        skills::sync(self, root)
    }
    fn layout_run_full(&self, cache: &LayoutCache) -> Result<u64, String> {
        layout::run_full(self, cache)
    }
    fn layout_snapshot(&self, cache: &LayoutCache) -> Result<snapshot::Snapshot, String> {
        self.transaction(|tx| layout::snapshot(tx, cache))
    }
    fn layout_delta(
        &self,
        cache: &LayoutCache,
        since: u64,
    ) -> Result<snapshot::DeltaResponse, String> {
        self.transaction(|tx| layout::delta(tx, cache, since))
    }
    fn layout_labels(&self, ids: &[String]) -> Result<Vec<(String, String)>, String> {
        self.transaction(|tx| layout::labels(tx, ids))
    }
    fn layout_edges(&self, ids: &[String]) -> Result<Vec<snapshot::EdgeRow>, String> {
        self.transaction(|tx| layout::edges(tx, ids))
    }
    fn layout_vectors(&self) -> Result<Vec<(String, Vec<f32>)>, String> {
        self.transaction(layout::vectors)
    }
    fn layout_place(&self, cache: &LayoutCache, id: &str, vector: &[f32]) -> Result<bool, String> {
        layout::place(self, cache, id, vector)
    }
    fn layout_place_stored(&self, cache: &LayoutCache, id: &str) {
        if cache.is_empty() {
            return;
        }
        if let Ok(Some(vector)) = self.transaction(|tx| layout::stored_vector(tx, id)) {
            let _ = layout::place(self, cache, id, &vector);
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    fn fixture_backend(&self) -> &dyn std::any::Any {
        self
    }
}

fn count(tx: &mut Transaction<'_>, table: &str) -> Result<u64, String> {
    Ok(tx
        .query_one(&format!("SELECT count(*) FROM {table}"), &[])
        .map_err(db_error)?
        .get::<_, i64>(0) as u64)
}
fn mark(tx: &mut Transaction<'_>, id: &str, reason: &str) -> Result<(), String> {
    tx.execute("INSERT INTO mem_vector_pending(mem_id,reason,queued_at) VALUES ($1,$2,$3) ON CONFLICT(mem_id) DO UPDATE SET reason=excluded.reason", &[&id,&reason,&crate::items::now()]).map_err(db_error)?;
    Ok(())
}
impl PostgresMemStore {
    fn engine_transaction<T>(
        &self,
        wait: bool,
        f: impl FnOnce(&mut Transaction<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = if wait {
            self.gate.lock()
        } else {
            self.gate
                .try_lock()
                .ok_or("memory is busy; embedder switch cannot complete now")?
        };
        self.transaction_locked(wait, f)
    }
}
