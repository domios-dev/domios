use super::{MemStore, WriteContext};
use crate::embedder::{ActiveEmbedder, EmbedderIdentity};
use crate::graph_layout::{cache::LayoutCache, snapshot};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

pub type LightCandidate = (String, i64, f64, String, Option<String>);
pub type PendingExtraction = (String, String, String, crate::MemKind);
pub type EngineStatus = (Option<EmbedderIdentity>, u64, u64);

#[derive(Debug, Clone)]
pub struct ShadowUnit {
    pub id: String,
    pub blueprint: bool,
    pub texts: Vec<(String, usize, String)>,
}

impl ShadowUnit {
    pub fn key(&self) -> String {
        format!("{}:{}", if self.blueprint { "b" } else { "m" }, self.id)
    }

    pub fn hash(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        for (slot, ordinal, text) in &self.texts {
            hasher.update(slot.as_bytes());
            hasher.update([0]);
            hasher.update(ordinal.to_le_bytes());
            hasher.update(text.as_bytes());
            hasher.update([0]);
        }
        hasher.finalize().to_vec()
    }
}

#[derive(Debug, Clone)]
pub struct TrackedSource {
    pub id: String,
    pub source_path: String,
    pub source_size: Option<i64>,
    pub source_mtime: Option<String>,
    pub tags_json: String,
}

pub struct SourceRefresh<'a> {
    pub body: Option<&'a str>,
    pub size: Option<i64>,
    pub mtime: Option<&'a str>,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceMode {
    Subprocess { db_path: std::path::PathBuf },
    InProcess,
}

pub trait EngineStore: Send + Sync {
    fn maintenance_mode(&self) -> MaintenanceMode;
    fn background_store(&self) -> Result<Arc<dyn MemStore>, String>;
    fn engine_status(&self, wait: bool) -> Result<EngineStatus, String>;
    fn engine_plan(&self, docs: u64) -> Result<crate::embedder::switch::SwitchPlan, String>;
    fn prepare_engine_switch(
        &self,
        ctx: &WriteContext,
        identity: &EmbedderIdentity,
        docs: u64,
        wait: bool,
    ) -> Result<crate::embedder::switch::SwitchPlan, String>;
    fn clear_shadow(&self, ctx: &WriteContext, wait: bool) -> Result<(), String>;
    fn adopt_published(
        &self,
        ctx: &WriteContext,
        active: &ActiveEmbedder,
        provider: crate::embedder::provider::SharedProvider,
    );
    fn reembed_item_targets(&self) -> Result<Vec<(String, String)>, String>;
    fn reembed_entity_targets(&self) -> Result<Vec<(String, String)>, String>;
    fn blueprint_reembed_targets(&self) -> Result<Vec<(String, String)>, String>;
    fn missing_item_vectors(&self) -> Vec<(String, String)>;
    fn missing_entity_vectors(&self) -> Result<Vec<(String, String)>, String>;
    fn pending_ids(&self) -> Result<Vec<String>, String>;
    fn mark_pending(&self, ctx: &WriteContext, id: &str, reason: &str) -> Result<(), String>;
    fn clear_pending(&self, ctx: &WriteContext, id: &str) -> Result<(), String>;
    fn pending_embed_text(&self, id: &str) -> Option<String>;
    fn shadow_stale(&self, stamp: &str) -> Result<(Vec<ShadowUnit>, u64), String>;
    fn shadow_checkpoint(
        &self,
        ctx: &WriteContext,
        stamp: &str,
        unit: &ShadowUnit,
        vectors: Option<&[u8]>,
        recovery: bool,
        current: &dyn Fn() -> bool,
    ) -> Result<(), String>;
    fn publish_shadow(
        &self,
        ctx: &WriteContext,
        target: &ActiveEmbedder,
        active: &ActiveEmbedder,
        stamp: &str,
        claim: &dyn Fn() -> bool,
    ) -> Result<bool, String>;
    fn checkpoint(&self) -> Result<(i64, i64, i64), String>;
    fn purge_precompact(
        &self,
        ctx: &WriteContext,
        id: &str,
        apply: bool,
        verified_deleted: bool,
        vacuum: bool,
    ) -> Result<crate::precompact_purge::PurgeReport, String>;
    fn tracked_sources(&self) -> Result<Vec<TrackedSource>, String>;
    fn refresh_source(
        &self,
        ctx: &WriteContext,
        expected: &TrackedSource,
        refresh: &SourceRefresh<'_>,
    ) -> Result<bool, String>;
    fn pending_extraction(&self, limit: usize) -> Result<Vec<PendingExtraction>, String>;
    fn bump_evidence(&self, ctx: &WriteContext, id: &str) -> Result<bool, String>;
    fn dream_candidates(&self, before: &str) -> Result<Vec<LightCandidate>, String>;
    fn dream_promote(
        &self,
        ctx: &WriteContext,
        candidate: &LightCandidate,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String>;
    fn dream_expire(&self, ctx: &WriteContext, kind: &str, days: i64) -> Result<u64, String>;
    fn dream_normalize(&self, ctx: &WriteContext) -> Result<serde_json::Value, String>;
    fn dream_worker(
        &self,
        ctx: &WriteContext,
        kind: &str,
        machine: &str,
        timeout: Duration,
    ) -> Result<serde_json::Value, String>;
    fn rem_snapshot(&self, blueprint_merges: usize) -> Result<String, String>;
    fn sync_skill_folders(
        &self,
        ctx: &WriteContext,
        root: &std::path::Path,
    ) -> codeterm_mem_store::skill_folders::SkillFolderReport;
    fn layout_run_full(&self, cache: &LayoutCache) -> Result<u64, String>;
    fn layout_snapshot(&self, cache: &LayoutCache) -> Result<snapshot::Snapshot, String>;
    fn layout_delta(
        &self,
        cache: &LayoutCache,
        since: u64,
    ) -> Result<snapshot::DeltaResponse, String>;
    fn layout_labels(&self, ids: &[String]) -> Result<Vec<(String, String)>, String>;
    fn layout_edges(&self, ids: &[String]) -> Result<Vec<snapshot::EdgeRow>, String>;
    fn layout_vectors(&self) -> Result<Vec<(String, Vec<f32>)>, String>;
    fn layout_place(&self, cache: &LayoutCache, id: &str, vector: &[f32]) -> Result<bool, String>;
    fn layout_place_stored(&self, cache: &LayoutCache, id: &str);
    #[cfg(any(test, feature = "test-support"))]
    fn fixture_backend(&self) -> &dyn std::any::Any;
}
