use super::HistoryBackfill;
use codeterm_types::agent::chat_types::ChatMsg;
use std::collections::{HashMap, HashSet, VecDeque};

pub const REREADABLE_CACHE_ROWS: usize = 400;
pub const REREADABLE_CACHE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct ItemCache {
    pub(super) items: VecDeque<ChatMsg>,
    rereadable: bool,
    pub(super) cached_bytes: usize,
    pub(super) positions: HashMap<String, i64>,
    head: i64,
    backfill_generation: u64,
    backfill: HistoryBackfill,
    history_pages: u32,
    pub(super) live_during_backfill: HashSet<String>,
    pub(super) history_origin: HashSet<String>,
    archive_tracking: bool,
    archive_dirty: HashSet<String>,
}
impl ItemCache {
    /// Start noting changed rows for the host's append-only session log.
    pub fn track_archive(&mut self) {
        self.archive_tracking = true;
    }
    /// The rows changed since the last call, oldest first.
    pub fn take_archive_rows(&mut self) -> Vec<ChatMsg> {
        let dirty = std::mem::take(&mut self.archive_dirty);
        let mut rows: Vec<ChatMsg> = dirty
            .iter()
            .filter_map(|id| self.get(id).cloned())
            .collect();
        rows.sort_by_key(|row| row.seq);
        rows
    }
    /// Put rows back after a failed append, so the next one retries them.
    pub fn mark_archive_rows(&mut self, rows: &[ChatMsg]) {
        if self.archive_tracking {
            self.archive_dirty
                .extend(rows.iter().map(|row| row.id.clone()));
        }
    }
    pub fn bound_rereadable(&mut self) {
        self.rereadable = true;
        self.cached_bytes = self.items.iter().map(Self::row_bytes).sum();
        self.evict();
    }
    pub fn retained(&self) -> (usize, usize) {
        (self.items.len(), self.cached_bytes)
    }
    fn row_bytes(row: &ChatMsg) -> usize {
        serde_json::to_vec(row).map_or(usize::MAX, |bytes| bytes.len())
    }
    fn evict(&mut self) {
        while self.rereadable
            && (self.items.len() > REREADABLE_CACHE_ROWS
                || self.cached_bytes > REREADABLE_CACHE_BYTES)
        {
            let Some(row) = self.items.pop_front() else {
                break;
            };
            self.cached_bytes = self.cached_bytes.saturating_sub(Self::row_bytes(&row));
            self.positions.remove(&row.id);
            self.history_origin.remove(&row.id);
            self.live_during_backfill.remove(&row.id);
            self.head += 1;
        }
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.positions
            .get(id)
            .map(|slot| (slot - self.head) as usize)
    }
    /// Starts a replay and drops rows a previous replay landed, keeping live and host rows,
    /// so every history row of this replay can sit below the surviving minimum.
    pub fn begin_backfill(&mut self) -> u64 {
        self.begin_cached_backfill();
        if !self.history_origin.is_empty() {
            let origin = std::mem::take(&mut self.history_origin);
            self.items.retain(|row| !origin.contains(&row.id));
            self.head = 0;
            self.positions = self
                .items
                .iter()
                .enumerate()
                .map(|(index, row)| (row.id.clone(), index as i64))
                .collect();
        }
        if self.rereadable {
            self.cached_bytes = self.items.iter().map(Self::row_bytes).sum();
        }
        self.backfill_generation
    }
    pub fn begin_cached_backfill(&mut self) -> u64 {
        self.backfill_generation = self.backfill_generation.wrapping_add(1);
        self.backfill = HistoryBackfill::Loading;
        self.history_pages = 0;
        self.live_during_backfill.clear();
        self.backfill_generation
    }
    pub fn backfill_current(&self, generation: u64) -> bool {
        self.backfill_generation == generation
    }
    pub fn finish_backfill(&mut self, generation: u64, outcome: HistoryBackfill) -> bool {
        if self.backfill_generation != generation {
            return false;
        }
        self.backfill = outcome;
        if outcome == HistoryBackfill::Complete {
            self.live_during_backfill.clear();
        }
        true
    }
    pub fn resume_backfill(&mut self, generation: u64) -> bool {
        if !self.backfill_current(generation) || self.backfill != HistoryBackfill::Failed {
            return false;
        }
        self.backfill = HistoryBackfill::Loading;
        true
    }
    pub fn backfill(&self) -> HistoryBackfill {
        self.backfill
    }
    pub fn history_pages(&self) -> u32 {
        self.history_pages
    }
    pub fn note_history_page(&mut self) {
        self.history_pages = self.history_pages.saturating_add(1);
    }
    /// Lands history rows older than every cached row without renumbering: known rows update
    /// in place unless a live delta superseded them, unknown rows take seqs below the minimum.
    pub fn reconcile_backfill(&mut self, page: Vec<ChatMsg>) -> usize {
        let mut landed = 0;
        let mut block: Vec<ChatMsg> = Vec::new();
        for mut item in page {
            if self.live_during_backfill.contains(&item.id) {
                continue;
            }
            if self.positions.contains_key(&item.id) {
                self.history_origin.insert(item.id.clone());
                self.reconcile(item);
                landed += 1;
                continue;
            }
            item = item.with_segments();
            match block.iter_mut().find(|row| row.id == item.id) {
                Some(row) => *row = item,
                None => block.push(item),
            }
        }
        landed += block.len();
        let base = self.items.front().map_or(0, |row| row.seq) - block.len() as i64;
        for (offset, mut item) in block.into_iter().enumerate().rev() {
            item.seq = base + offset as i64;
            self.head -= 1;
            self.positions.insert(item.id.clone(), self.head);
            self.history_origin.insert(item.id.clone());
            if self.rereadable {
                self.cached_bytes = self.cached_bytes.saturating_add(Self::row_bytes(&item));
            }
            self.items.push_front(item);
        }
        self.evict();
        landed
    }
    pub fn reconcile_live(&mut self, mut item: ChatMsg, at: u64) -> ChatMsg {
        if self.backfill != HistoryBackfill::Complete {
            self.live_during_backfill.insert(item.id.clone());
        }
        if let Some(status) = item.tool_status.as_deref() {
            let previous = self.index(&item.id).map(|index| &self.items[index]);
            item.tool_started_at_ms = item
                .tool_started_at_ms
                .or_else(|| previous.and_then(|row| row.tool_started_at_ms))
                .or_else(|| (status == "running").then_some(at));
            if status != "running" {
                item.tool_completed_at_ms = item
                    .tool_completed_at_ms
                    .or_else(|| previous.and_then(|row| row.tool_completed_at_ms))
                    .or(Some(at));
            }
        }
        self.reconcile(item)
    }
    pub fn reconcile(&mut self, mut item: ChatMsg) -> ChatMsg {
        item = item.with_segments();
        if self.archive_tracking {
            self.archive_dirty.insert(item.id.clone());
        }
        if let Some(index) = self.index(&item.id) {
            item.seq = self.items[index].seq;
            if self.rereadable {
                self.cached_bytes = self
                    .cached_bytes
                    .saturating_sub(Self::row_bytes(&self.items[index]))
                    .saturating_add(Self::row_bytes(&item));
            }
            self.items[index] = item.clone();
        } else {
            item.seq = self.items.back().map_or(0, |row| row.seq + 1);
            self.positions
                .insert(item.id.clone(), self.head + self.items.len() as i64);
            if self.rereadable {
                self.cached_bytes = self.cached_bytes.saturating_add(Self::row_bytes(&item));
            }
            self.items.push_back(item.clone());
        }
        self.evict();
        item
    }
    pub fn next_ordinal(&self) -> u64 {
        self.items
            .back()
            .map_or(0, |row| row.seq.saturating_add(1).max(0) as u64)
    }
    pub fn restore_transcript(&mut self, rows: Vec<ChatMsg>) {
        for row in rows {
            if self.positions.contains_key(&row.id) {
                continue;
            }
            let seq = row.seq;
            let id = row.id.clone();
            self.reconcile(row);
            // A restored row came from disk, so it has nothing new to append.
            self.archive_dirty.remove(&id);
            if let Some(index) = self.index(&id) {
                self.items[index].seq = seq;
            }
        }
    }
    pub fn snapshot(&self) -> Vec<ChatMsg> {
        self.items.iter().cloned().collect()
    }
    pub fn get(&self, id: &str) -> Option<&ChatMsg> {
        self.index(id).map(|index| &self.items[index])
    }
    pub fn has_turn(&self, turn: &str) -> bool {
        self.items
            .iter()
            .any(|item| item.tool_flow_id.as_deref() == Some(turn))
    }
    pub fn truncate_after_turn(&mut self, turn: &str) -> bool {
        let Some(end) = self
            .items
            .iter()
            .rposition(|item| item.tool_flow_id.as_deref() == Some(turn))
        else {
            return false;
        };
        while self.items.len() > end + 1 {
            if let Some(item) = self.items.pop_back() {
                self.positions.remove(&item.id);
                self.history_origin.remove(&item.id);
                self.live_during_backfill.remove(&item.id);
                if self.rereadable {
                    self.cached_bytes = self.cached_bytes.saturating_sub(Self::row_bytes(&item));
                }
            }
        }
        true
    }
    pub fn last_message_id(&self) -> Option<&str> {
        self.items.back().map(|row| row.id.as_str())
    }

    pub fn tail(&self, limit: usize) -> Vec<ChatMsg> {
        self.items
            .range(self.items.len().saturating_sub(limit)..)
            .cloned()
            .collect()
    }
}
