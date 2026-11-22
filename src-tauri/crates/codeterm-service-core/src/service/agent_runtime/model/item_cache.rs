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
    user_source_ids: HashSet<String>,
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
    pub(crate) fn handoff_origin(
        &self,
    ) -> Option<crate::service::tab_chat::runtime_stored::handoff_origin::HandoffOrigin> {
        self.items.iter().find_map(
            crate::service::tab_chat::runtime_stored::handoff_origin::HandoffOrigin::recorded,
        )
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
            if let Some(source) = Self::user_source_id(&row) {
                self.user_source_ids.remove(source);
            }
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
    fn user_source_id(row: &ChatMsg) -> Option<&str> {
        row.runtime_source_item_id
            .as_deref()
            .filter(|_| row.kind == "user")
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
            // A history echo preserves the host row's body, routing metadata and sequence.
            if item.kind == "user"
                && (self.user_source_ids.contains(&item.id)
                    || self.get(&item.id).and_then(Self::user_source_id).is_some())
            {
                continue;
            }
            let provider_terminal_tool = item.kind == "tool_call"
                && item.tool_completed_at_ms.is_some()
                && item
                    .tool_status
                    .as_deref()
                    .is_some_and(|status| status != "running")
                && self.get(&item.id).is_some_and(|current| {
                    current.tool_status.as_deref() == Some("interrupted")
                        && current.tool_result.is_none()
                });
            if self.live_during_backfill.contains(&item.id) && !provider_terminal_tool {
                continue;
            }
            item.runtime_source_item_id = None;
            if self.positions.contains_key(&item.id) {
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
        if item.timestamp.is_none() {
            item.timestamp = self
                .index(&item.id)
                .and_then(|index| self.items[index].timestamp.clone());
        }
        if let Some(status) = item.tool_status.as_deref() {
            let previous = self.index(&item.id).map(|index| &self.items[index]);
            if let Some(previous) = previous
                .filter(|previous| item.kind == "tool_call" && previous.tool_id == item.tool_id)
            {
                item.tool_kind = item.tool_kind.or_else(|| previous.tool_kind.clone());
                item.tool_name = item.tool_name.or_else(|| previous.tool_name.clone());
                item.tool_input = item.tool_input.or_else(|| previous.tool_input.clone());
            }
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
            if let Some(previous) = Self::user_source_id(&self.items[index]) {
                if Some(previous) != Self::user_source_id(&item) {
                    self.user_source_ids.remove(previous);
                }
            }
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
        if let Some(source) = Self::user_source_id(&item) {
            self.user_source_ids.insert(source.into());
        }
        self.evict();
        item
    }
    pub fn next_ordinal(&self) -> u64 {
        self.items
            .back()
            .map_or(0, |row| row.seq.saturating_add(1).max(0) as u64)
    }
    pub fn patch_terminal_output(&mut self, terminal_id: &str, output: String) -> Option<ChatMsg> {
        let index = self.items.iter().rposition(|row| {
            row.kind == "tool_call"
                && row.tool_status.as_deref() == Some("running")
                && row.tool_background_task_id.as_deref() == Some(terminal_id)
        })?;
        if self.items[index].tool_result.as_deref() == Some(output.as_str()) {
            return None;
        }
        let mut updated = self.items[index].clone();
        updated.tool_result = Some(output);
        Some(self.reconcile(updated))
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
    /// Completion evidence borrows a bounded recent window; it never clones transcript rows.
    pub(crate) fn terminal_evidence(&self) -> (String, bool) {
        use serde::Serialize;
        if self.items.len() <= TERMINAL_ROWS {
            let mut budget = TerminalBudget {
                scalars: TERMINAL_BYTES,
                nodes: TERMINAL_BYTES,
                depth: 0,
            };
            if self.items.serialize(&mut budget).is_ok() {
                let mut output = TerminalWriter(Vec::new());
                if serde_json::to_writer(&mut output, &self.items).is_ok() {
                    return (String::from_utf8(output.0).expect("JSON is UTF8"), true);
                }
            }
        }
        let mut remaining = TERMINAL_BYTES - PARTIAL_TERMINAL.len();
        let mut pieces = Vec::new();
        for row in self.items.iter().rev().take(TERMINAL_ROWS) {
            if remaining <= 1 {
                break;
            }
            let take = remaining.saturating_sub(1).min(row.content.len());
            let mut start = row.content.len() - take;
            while !row.content.is_char_boundary(start) {
                start += 1;
            }
            let part = &row.content[start..];
            remaining -= part.len() + 1;
            pieces.push(part);
        }
        let mut output = String::with_capacity(TERMINAL_BYTES);
        output.push_str(PARTIAL_TERMINAL);
        for part in pieces.into_iter().rev() {
            output.push_str(part);
            output.push('\n');
        }
        (output, false)
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
                if let Some(source) = Self::user_source_id(&item) {
                    self.user_source_ids.remove(source);
                }
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

const TERMINAL_ROWS: usize = 32;
const TERMINAL_BYTES: usize = 8 * 1024;
const TERMINAL_DEPTH: usize = 8;
const PARTIAL_TERMINAL: &str = "PARTIAL recent content projection; metadata omitted; full transcript bytecount/digest unavailable\n";

struct TerminalWriter(Vec<u8>);
impl std::io::Write for TerminalWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > TERMINAL_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("terminal evidence byte cap"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Admission only: reject large scalars/containers before the JSON escaper can traverse them.
struct TerminalBudget {
    scalars: usize,
    nodes: usize,
    depth: usize,
}
struct TerminalFields<'a>(&'a mut TerminalBudget);
impl TerminalFields<'_> {
    fn visit<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), serde_json::Error> {
        value.serialize(&mut *self.0)
    }
}
fn terminal_limit() -> serde_json::Error {
    <serde_json::Error as serde::ser::Error>::custom("terminal evidence traversal cap")
}
impl TerminalBudget {
    fn node(&mut self) -> Result<(), serde_json::Error> {
        self.nodes = self.nodes.checked_sub(1).ok_or_else(terminal_limit)?;
        Ok(())
    }
    fn scalar(&mut self, bytes: usize) -> Result<(), serde_json::Error> {
        self.scalars = self.scalars.checked_sub(bytes).ok_or_else(terminal_limit)?;
        self.node()
    }
    fn fields(&mut self) -> Result<TerminalFields<'_>, serde_json::Error> {
        self.node()?;
        if self.depth >= TERMINAL_DEPTH {
            return Err(terminal_limit());
        }
        self.depth += 1;
        Ok(TerminalFields(self))
    }
}
impl Drop for TerminalFields<'_> {
    fn drop(&mut self) {
        self.0.depth -= 1;
    }
}

macro_rules! terminal_primitive {
    ($($method:ident: $kind:ty),* $(,)?) => { $(fn $method(self, _: $kind) -> Result<(), Self::Error> { self.node() })* };
}
impl<'a> serde::Serializer for &'a mut TerminalBudget {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = TerminalFields<'a>;
    type SerializeTuple = TerminalFields<'a>;
    type SerializeTupleStruct = TerminalFields<'a>;
    type SerializeTupleVariant = TerminalFields<'a>;
    type SerializeMap = TerminalFields<'a>;
    type SerializeStruct = TerminalFields<'a>;
    type SerializeStructVariant = TerminalFields<'a>;
    terminal_primitive!(serialize_bool:bool, serialize_i8:i8, serialize_i16:i16, serialize_i32:i32, serialize_i64:i64, serialize_i128:i128,
        serialize_u8:u8, serialize_u16:u16, serialize_u32:u32, serialize_u64:u64, serialize_u128:u128, serialize_f32:f32, serialize_f64:f64, serialize_char:char);
    fn serialize_str(self, value: &str) -> Result<(), Self::Error> {
        self.scalar(value.len())?;
        let escaped_extra: usize = value
            .bytes()
            .map(|byte| match byte {
                b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 1,
                0x00..=0x1f => 5,
                _ => 0,
            })
            .sum();
        if escaped_extra > 0 {
            self.scalars = self
                .scalars
                .checked_sub(escaped_extra)
                .ok_or_else(terminal_limit)?;
        }
        Ok(())
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<(), Self::Error> {
        self.scalar(value.len())
    }
    fn serialize_none(self) -> Result<(), Self::Error> {
        self.node()
    }
    fn serialize_some<T: ?Sized + serde::Serialize>(self, value: &T) -> Result<(), Self::Error> {
        self.node()?;
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Self::Error> {
        self.node()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Self::Error> {
        self.node()
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<(), Self::Error> {
        self.scalar(variant.len())
    }
    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let mut fields = self.fields()?;
        fields.visit(value)
    }
    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.scalar(variant.len())?;
        let mut fields = self.fields()?;
        fields.visit(value)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.fields()
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.fields()
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.fields()
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.scalar(variant.len())?;
        self.fields()
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        self.fields()
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.fields()
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.scalar(variant.len())?;
        self.fields()
    }
    fn collect_str<T: ?Sized + std::fmt::Display>(self, _: &T) -> Result<(), Self::Error> {
        Err(terminal_limit())
    }
}
macro_rules! terminal_sequence {
    ($trait:ident, $method:ident) => {
        impl serde::ser::$trait for TerminalFields<'_> {
            type Ok = ();
            type Error = serde_json::Error;
            fn $method<T: ?Sized + serde::Serialize>(
                &mut self,
                value: &T,
            ) -> Result<(), Self::Error> {
                value.serialize(&mut *self.0)
            }
            fn end(self) -> Result<(), Self::Error> {
                Ok(())
            }
        }
    };
}
terminal_sequence!(SerializeSeq, serialize_element);
terminal_sequence!(SerializeTuple, serialize_element);
terminal_sequence!(SerializeTupleStruct, serialize_field);
terminal_sequence!(SerializeTupleVariant, serialize_field);
impl serde::ser::SerializeMap for TerminalFields<'_> {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_key<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(&mut *self.0)
    }
    fn serialize_value<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(&mut *self.0)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}
macro_rules! terminal_struct {
    ($trait:ident) => {
        impl serde::ser::$trait for TerminalFields<'_> {
            type Ok = ();
            type Error = serde_json::Error;
            fn serialize_field<T: ?Sized + serde::Serialize>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result<(), Self::Error> {
                self.0.scalar(key.len())?;
                value.serialize(&mut *self.0)
            }
            fn end(self) -> Result<(), Self::Error> {
                Ok(())
            }
        }
    };
}
terminal_struct!(SerializeStruct);
terminal_struct!(SerializeStructVariant);
