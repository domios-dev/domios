use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::store::{
    invalid_data, invalid_input, open_private, raw_path, read_private, sync_parent, visit_raw,
    write_private, StoreState, MAX_RECORD_BYTES, RAW_SEGMENT_BYTES,
};
use super::{SegmentSummary, TranscriptRange, TranscriptRow, TranscriptStore};

pub const DEFAULT_RAW_BUDGET_BYTES: u64 = 50 * 1024 * 1024;
const MAX_SUMMARY_BYTES: usize = 16 * 1024;
const MAX_EVICTIONS_PER_CALL: usize = 64;

impl TranscriptStore {
    /// Recover pending jobs after daemon restart, even when a seal event was not delivered.
    pub fn pending_summaries(&self, limit: usize) -> io::Result<Vec<TranscriptRange>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        Ok(state
            .segments
            .iter()
            .enumerate()
            .filter(|(index, segment)| {
                segment.has_raw() && state.is_sealed(*index) && !segment.summarized
            })
            .take(limit.min(MAX_EVICTIONS_PER_CALL))
            .map(|(_, segment)| segment.range)
            .collect())
    }

    pub fn read_sealed(&self, range: TranscriptRange) -> io::Result<Vec<TranscriptRow>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let index = sealed_index(&state, range)?;
        let segment = &state.segments[index];
        if !segment.has_raw() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Sealed transcript raw has been evicted",
            ));
        }
        let mut rows = Vec::new();
        visit_raw(&self.inner.root, segment, |row| {
            rows.push(row);
            Ok(true)
        })?;
        Ok(rows)
    }

    pub fn write_summary(&self, summary: SegmentSummary) -> io::Result<()> {
        if summary.text.trim().is_empty() || summary.text.len() > MAX_SUMMARY_BYTES {
            return Err(invalid_input("Empty or oversized transcript summary"));
        }
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        let index = sealed_index(&state, summary.range)?;
        if let Some(existing) = read_summary(&self.inner.root, summary.range)? {
            return if existing == summary {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Transcript summary already committed",
                ))
            };
        }
        let path = summary_path(&self.inner.root, summary.range);
        if let Err(error) = write_private(&path, &summary, MAX_RECORD_BYTES) {
            state.fence_writes();
            return Err(error);
        }
        state.segments[index].summarized = true;
        Ok(())
    }

    pub fn raw_bytes(&self) -> u64 {
        self.inner
            .state
            .lock()
            .segments
            .iter()
            .map(|segment| segment.bytes)
            .sum()
    }

    /// A full result can be followed by another bounded eviction pass.
    pub fn evict_to_budget(&self) -> io::Result<Vec<TranscriptRange>> {
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        state.binding.require_open()?;
        self.evict_locked(&mut state, self.inner.raw_budget_bytes)
    }

    pub(super) fn evict_locked(
        &self,
        state: &mut StoreState,
        budget: u64,
    ) -> io::Result<Vec<TranscriptRange>> {
        if state.readers != 0 {
            return Ok(Vec::new());
        }
        let mut remaining = state
            .segments
            .iter()
            .map(|segment| segment.bytes)
            .sum::<u64>();
        if remaining <= budget {
            return Ok(Vec::new());
        }
        let Some(cursor) = state
            .binding
            .active_cursor(&self.inner.root, state.last_seq())?
        else {
            return Ok(Vec::new());
        };

        let mut candidates = Vec::new();
        let mut manifest = state.manifest.clone();
        for (index, segment) in state.segments.iter().enumerate() {
            if segment.range.to_seq > cursor.acked_seq
                || remaining <= budget
                || candidates.len() == MAX_EVICTIONS_PER_CALL
            {
                break;
            }
            if !segment.has_raw() || !segment.summarized || !state.is_sealed(index) {
                continue;
            }
            require_summary(&self.inner.root, segment.range)?;
            let path = raw_path(&self.inner.root, segment.range.from_seq);
            let raw = open_private(&path, RAW_SEGMENT_BYTES)?;
            if raw.metadata()?.len() != segment.bytes {
                return Err(invalid_data("Transcript raw changed before eviction"));
            }
            manifest
                .cold_ranges
                .insert(segment.range.from_seq, segment.range);
            remaining -= segment.bytes;
            candidates.push((index, path));
        }

        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        // Persist sequence and summary provenance before raw unlink.
        self.save_manifest(state, manifest)?;
        let mut evicted = Vec::new();
        for (index, path) in candidates {
            fs::remove_file(&path)?;
            state.segments[index].bytes = 0;
            evicted.push(state.segments[index].range);
        }
        if !evicted.is_empty() {
            sync_parent(&raw_path(&self.inner.root, evicted[0].from_seq))?;
        }
        Ok(evicted)
    }
}

fn sealed_index(state: &StoreState, range: TranscriptRange) -> io::Result<usize> {
    let index = state.segment_index(range.from_seq);
    if state
        .segments
        .get(index)
        .is_none_or(|segment| segment.range != range)
        || !state.is_sealed(index)
    {
        return Err(invalid_input("Exact sealed transcript range required"));
    }
    Ok(index)
}

pub(super) fn require_summary(root: &Path, range: TranscriptRange) -> io::Result<SegmentSummary> {
    read_summary(root, range)?.ok_or_else(|| invalid_data("Durable transcript summary is missing"))
}

pub(super) fn read_summary(
    root: &Path,
    range: TranscriptRange,
) -> io::Result<Option<SegmentSummary>> {
    let summary = read_private::<SegmentSummary>(&summary_path(root, range), MAX_RECORD_BYTES)?;
    if summary.as_ref().is_some_and(|summary| {
        summary.range != range
            || summary.text.trim().is_empty()
            || summary.text.len() > MAX_SUMMARY_BYTES
    }) {
        return Err(invalid_data("Invalid transcript summary"));
    }
    Ok(summary)
}

fn summary_path(root: &Path, range: TranscriptRange) -> PathBuf {
    root.join("summaries")
        .join(format!("{:020}.json", range.from_seq))
}
