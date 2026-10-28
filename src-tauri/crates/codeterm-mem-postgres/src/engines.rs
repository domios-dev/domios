use crate::PostgresMemStore;
use codeterm_mem::backend::{engines::*, MemStore, WriteContext};
use codeterm_mem::embedder::{ActiveEmbedder, EmbedderIdentity};
use codeterm_mem::graph_layout::{cache::LayoutCache, snapshot};
use std::{sync::Arc, time::Duration};

impl EngineStore for PostgresMemStore {
    fn maintenance_mode(&self) -> MaintenanceMode {
        MaintenanceMode::InProcess
    }
    fn background_store(&self) -> Result<Arc<dyn MemStore>, String> {
        Err(crate::unsupported("background_store"))
    }
    fn engine_status(&self, _wait: bool) -> Result<EngineStatus, String> {
        Err(crate::unsupported("engine_status"))
    }
    fn engine_plan(
        &self,
        _docs: u64,
    ) -> Result<codeterm_mem::embedder::switch::SwitchPlan, String> {
        Err(crate::unsupported("engine_plan"))
    }
    fn prepare_engine_switch(
        &self,
        _ctx: &WriteContext,
        _identity: &EmbedderIdentity,
        _docs: u64,
        _wait: bool,
    ) -> Result<codeterm_mem::embedder::switch::SwitchPlan, String> {
        Err(crate::unsupported("prepare_engine_switch"))
    }
    fn clear_shadow(&self, _ctx: &WriteContext, _wait: bool) -> Result<(), String> {
        Err(crate::unsupported("clear_shadow"))
    }
    fn adopt_published(
        &self,
        _ctx: &WriteContext,
        _active: &ActiveEmbedder,
        _provider: codeterm_mem::embedder::provider::SharedProvider,
    ) {
        std::panic::panic_any(crate::UnsupportedOperation {
            method: "adopt_published",
        })
    }
    fn reembed_item_targets(&self) -> Result<Vec<(String, String)>, String> {
        Err(crate::unsupported("reembed_item_targets"))
    }
    fn reembed_entity_targets(&self) -> Result<Vec<(String, String)>, String> {
        Err(crate::unsupported("reembed_entity_targets"))
    }
    fn blueprint_reembed_targets(&self) -> Result<Vec<(String, String)>, String> {
        Err(crate::unsupported("blueprint_reembed_targets"))
    }
    fn missing_item_vectors(&self) -> Vec<(String, String)> {
        std::panic::panic_any(crate::UnsupportedOperation {
            method: "missing_item_vectors",
        })
    }
    fn missing_entity_vectors(&self) -> Result<Vec<(String, String)>, String> {
        Err(crate::unsupported("missing_entity_vectors"))
    }
    fn pending_ids(&self) -> Result<Vec<String>, String> {
        Err(crate::unsupported("pending_ids"))
    }
    fn mark_pending(&self, _ctx: &WriteContext, _id: &str, _reason: &str) -> Result<(), String> {
        Err(crate::unsupported("mark_pending"))
    }
    fn clear_pending(&self, _ctx: &WriteContext, _id: &str) -> Result<(), String> {
        Err(crate::unsupported("clear_pending"))
    }
    fn pending_embed_text(&self, _id: &str) -> Option<String> {
        std::panic::panic_any(crate::UnsupportedOperation {
            method: "pending_embed_text",
        })
    }
    fn shadow_stale(&self, _stamp: &str) -> Result<(Vec<ShadowUnit>, u64), String> {
        Err(crate::unsupported("shadow_stale"))
    }
    fn shadow_checkpoint(
        &self,
        _ctx: &WriteContext,
        _stamp: &str,
        _unit: &ShadowUnit,
        _vectors: Option<&[u8]>,
        _recovery: bool,
        _current: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        Err(crate::unsupported("shadow_checkpoint"))
    }
    fn publish_shadow(
        &self,
        _ctx: &WriteContext,
        _target: &ActiveEmbedder,
        _active: &ActiveEmbedder,
        _stamp: &str,
        _claim: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        Err(crate::unsupported("publish_shadow"))
    }
    fn checkpoint(&self) -> Result<(i64, i64, i64), String> {
        Err(crate::unsupported("checkpoint"))
    }
    fn purge_precompact(
        &self,
        _ctx: &WriteContext,
        _id: &str,
        _apply: bool,
        _verified_deleted: bool,
        _vacuum: bool,
    ) -> Result<codeterm_mem::precompact_purge::PurgeReport, String> {
        Err(crate::unsupported("purge_precompact"))
    }
    fn tracked_sources(&self) -> Result<Vec<TrackedSource>, String> {
        Err(crate::unsupported("tracked_sources"))
    }
    fn refresh_source(
        &self,
        _ctx: &WriteContext,
        _expected: &TrackedSource,
        _refresh: &SourceRefresh<'_>,
    ) -> Result<bool, String> {
        Err(crate::unsupported("refresh_source"))
    }
    fn pending_extraction(&self, _limit: usize) -> Result<Vec<PendingExtraction>, String> {
        Err(crate::unsupported("pending_extraction"))
    }
    fn bump_evidence(&self, _ctx: &WriteContext, _id: &str) -> Result<bool, String> {
        Err(crate::unsupported("bump_evidence"))
    }
    fn dream_candidates(&self, _before: &str) -> Result<Vec<LightCandidate>, String> {
        Err(crate::unsupported("dream_candidates"))
    }
    fn dream_promote(
        &self,
        _ctx: &WriteContext,
        _candidate: &LightCandidate,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String> {
        Err(crate::unsupported("dream_promote"))
    }
    fn dream_expire(&self, _ctx: &WriteContext, _kind: &str, _days: i64) -> Result<u64, String> {
        Err(crate::unsupported("dream_expire"))
    }
    fn dream_normalize(&self, _ctx: &WriteContext) -> Result<serde_json::Value, String> {
        Err(crate::unsupported("dream_normalize"))
    }
    fn dream_worker(
        &self,
        _ctx: &WriteContext,
        _kind: &str,
        _machine: &str,
        _timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        Err(crate::unsupported("dream_worker"))
    }
    fn rem_snapshot(&self, _blueprint_merges: usize) -> Result<String, String> {
        Err(crate::unsupported("rem_snapshot"))
    }
    fn sync_skill_folders(
        &self,
        _ctx: &WriteContext,
        _root: &std::path::Path,
    ) -> codeterm_mem_store::skill_folders::SkillFolderReport {
        std::panic::panic_any(crate::UnsupportedOperation {
            method: "sync_skill_folders",
        })
    }
    fn layout_run_full(&self, _cache: &LayoutCache) -> Result<u64, String> {
        Err(crate::unsupported("layout_run_full"))
    }
    fn layout_snapshot(&self, _cache: &LayoutCache) -> Result<snapshot::Snapshot, String> {
        Err(crate::unsupported("layout_snapshot"))
    }
    fn layout_delta(
        &self,
        _cache: &LayoutCache,
        _since: u64,
    ) -> Result<snapshot::DeltaResponse, String> {
        Err(crate::unsupported("layout_delta"))
    }
    fn layout_labels(&self, _ids: &[String]) -> Result<Vec<(String, String)>, String> {
        Err(crate::unsupported("layout_labels"))
    }
    fn layout_edges(&self, _ids: &[String]) -> Result<Vec<snapshot::EdgeRow>, String> {
        Err(crate::unsupported("layout_edges"))
    }
    fn layout_vectors(&self) -> Result<Vec<(String, Vec<f32>)>, String> {
        Err(crate::unsupported("layout_vectors"))
    }
    fn layout_place(
        &self,
        _cache: &LayoutCache,
        _id: &str,
        _vector: &[f32],
    ) -> Result<bool, String> {
        Err(crate::unsupported("layout_place"))
    }
    fn layout_place_stored(&self, _cache: &LayoutCache, _id: &str) {
        std::panic::panic_any(crate::UnsupportedOperation {
            method: "layout_place_stored",
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    fn fixture_backend(&self) -> &dyn std::any::Any {
        self
    }
}
