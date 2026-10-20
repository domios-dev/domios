//! Per-item fair schedule of pending embedding chunks.
//!
//! Each item keeps its own chunk backlog. Units are handed out round-robin
//! across items, and a newly submitted item goes to the front of the rotation,
//! so a small item is embedded next regardless of any large item's backlog.

use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbedUnit {
    pub id: String,
    pub generation: u64,
    pub chunk_index: usize,
    pub is_last: bool,
    pub text: String,
}

#[derive(Debug)]
struct PendingItem {
    generation: u64,
    next_index: usize,
    chunks: VecDeque<String>,
}

#[derive(Debug, Default)]
pub(crate) struct EmbedSchedule {
    items: HashMap<String, PendingItem>,
    rotation: VecDeque<String>,
    next_generation: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SubmitOutcome {
    Queued { generation: u64, superseded: bool },
    Full,
}

impl EmbedSchedule {
    pub fn is_idle(&self) -> bool {
        self.rotation.is_empty()
    }

    pub fn pending_items(&self) -> usize {
        self.rotation.len()
    }

    #[cfg(test)]
    pub fn remaining_chunks(&self, id: &str) -> usize {
        self.items.get(id).map_or(0, |item| item.chunks.len())
    }

    /// Queue `chunks` for `id`, superseding every pending or in-flight chunk of
    /// an older generation. `capacity` bounds the number of distinct items.
    pub fn submit(&mut self, id: &str, chunks: Vec<String>, capacity: usize) -> SubmitOutcome {
        let superseded = self.items.contains_key(id);
        if !superseded && self.items.len() >= capacity {
            return SubmitOutcome::Full;
        }
        self.next_generation += 1;
        let generation = self.next_generation;
        if chunks.is_empty() {
            self.cancel(id);
            return SubmitOutcome::Queued {
                generation,
                superseded,
            };
        }
        self.items.insert(
            id.to_string(),
            PendingItem {
                generation,
                next_index: 0,
                chunks: chunks.into(),
            },
        );
        if !self.rotation.iter().any(|queued| queued == id) {
            self.rotation.push_front(id.to_string());
        }
        SubmitOutcome::Queued {
            generation,
            superseded,
        }
    }

    /// Drop every pending chunk of `id`; a unit already handed out becomes
    /// stale. Returns how many chunks were discarded.
    pub fn cancel(&mut self, id: &str) -> usize {
        self.rotation.retain(|queued| queued != id);
        self.items.remove(id).map_or(0, |item| item.chunks.len())
    }

    /// Next unit in round-robin order. The worker must pass it to `finish`.
    pub fn next_unit(&mut self) -> Option<EmbedUnit> {
        while let Some(id) = self.rotation.pop_front() {
            let Some(item) = self.items.get_mut(&id) else {
                continue;
            };
            let Some(text) = item.chunks.pop_front() else {
                continue;
            };
            let unit = EmbedUnit {
                id: id.clone(),
                generation: item.generation,
                chunk_index: item.next_index,
                is_last: item.chunks.is_empty(),
                text,
            };
            item.next_index += 1;
            if !item.chunks.is_empty() {
                self.rotation.push_back(id);
            }
            return Some(unit);
        }
        None
    }

    /// Whether `unit` still belongs to the live generation of its item: not
    /// cancelled and not superseded by a newer submit.
    pub fn is_current(&self, unit: &EmbedUnit) -> bool {
        self.items
            .get(&unit.id)
            .is_some_and(|item| item.generation == unit.generation)
    }

    /// Drop the rest of `unit`'s generation (its item vanished); a newer
    /// generation of the same id is left alone. Returns the discarded chunks.
    pub fn abandon(&mut self, unit: &EmbedUnit) -> usize {
        if self.is_current(unit) {
            self.cancel(&unit.id)
        } else {
            0
        }
    }

    /// Retire a handed-out unit; the item is forgotten once its last unit is done.
    pub fn finish(&mut self, unit: &EmbedUnit) {
        let drained = self
            .items
            .get(&unit.id)
            .is_some_and(|item| item.generation == unit.generation && item.chunks.is_empty());
        if drained {
            self.items.remove(&unit.id);
        }
    }

    #[cfg(test)]
    pub fn tracked_items(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests;
