use crate::MemManager;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PurgeReport {
    pub id: String,
    pub dry_run: bool,
    pub provenance: &'static str,
    pub history_rows: usize,
    pub history_rows_removed: usize,
    pub history_body_bytes_removed: u64,
    pub history_body_bytes: u64,
    pub vectors_removed: usize,
    pub fts_rows_removed: usize,
    pub blob_candidates: usize,
    pub blobs_removed: usize,
    pub blob_bytes_unlinked: u64,
    pub blobs_already_absent: usize,
    pub shared_blobs_retained: usize,
    pub logical_purge_complete: bool,
    pub blob_cleanup_complete: bool,
    pub vacuum: String,
    pub fts_compaction: String,
    pub wal_checkpoint: String,
    pub secure_erasure: bool,
    pub limitations: &'static str,
    pub errors: Vec<String>,
}

impl MemManager {
    pub fn purge_precompact(
        &self,
        id: &str,
        apply: bool,
        verified_deleted: bool,
        vacuum: bool,
    ) -> Result<PurgeReport, String> {
        self.store
            .purge_precompact(&Self::write_context(), id, apply, verified_deleted, vacuum)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
use crate::backend::sqlite::engines::purge::{blob_name, MAX_BODY_BYTES};
#[cfg(test)]
use rusqlite::params;
