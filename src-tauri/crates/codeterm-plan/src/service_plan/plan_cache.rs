//! Bounded recent-use cache of loaded plans, and the by-id load seam of `PlanStore`.

use super::{Plan, PlanError, PlanStore};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub(super) const PLAN_CACHE_CAPACITY: usize = 8;

pub(super) type PlanHandle = Arc<Mutex<Plan>>;

struct Slot {
    plan: PlanHandle,
    last_used: u64,
}

#[derive(Default)]
struct Slots {
    entries: HashMap<String, Slot>,
    clock: u64,
    /// Bumped by every commit and eviction, so a load that started before one is not admitted.
    epoch: u64,
}

pub(super) struct PlanCache {
    capacity: usize,
    slots: Mutex<Slots>,
}

impl PlanCache {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            slots: Mutex::new(Slots::default()),
        }
    }

    pub(super) fn get(&self, id: &str) -> Option<PlanHandle> {
        let mut slots = self.slots.lock();
        let tick = slots.next_tick();
        let slot = slots.entries.get_mut(id)?;
        slot.last_used = tick;
        Some(slot.plan.clone())
    }

    /// Read before a SQL load and hand back to `admit`.
    pub(super) fn epoch(&self) -> u64 {
        self.slots.lock().epoch
    }

    /// Caches a plan loaded since `epoch`. The handle callers must use comes back: the one
    /// already cached, or the loaded one (left uncached when a commit or eviction raced the load).
    pub(super) fn admit(&self, id: &str, loaded: PlanHandle, epoch: u64) -> PlanHandle {
        let mut slots = self.slots.lock();
        let tick = slots.next_tick();
        if let Some(slot) = slots.entries.get_mut(id) {
            slot.last_used = tick;
            return slot.plan.clone();
        }
        if slots.epoch != epoch {
            return loaded;
        }
        slots.entries.insert(
            id.to_string(),
            Slot {
                plan: loaded.clone(),
                last_used: tick,
            },
        );
        slots.trim(self.capacity);
        loaded
    }

    /// Makes `plan` the cached handle of `id` after its write committed.
    pub(super) fn commit(&self, id: &str, plan: &PlanHandle) {
        let mut slots = self.slots.lock();
        let tick = slots.next_tick();
        slots.epoch += 1;
        slots.entries.insert(
            id.to_string(),
            Slot {
                plan: plan.clone(),
                last_used: tick,
            },
        );
        slots.trim(self.capacity);
    }

    /// Drops one plan so the next read loads it from SQL.
    pub(super) fn evict(&self, id: &str) -> Option<PlanHandle> {
        let mut slots = self.slots.lock();
        slots.epoch += 1;
        slots.entries.remove(id).map(|slot| slot.plan)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn clear(&self) {
        let mut slots = self.slots.lock();
        slots.epoch += 1;
        slots.entries.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn cached_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.slots.lock().entries.keys().cloned().collect();
        ids.sort();
        ids
    }
}

impl Slots {
    fn next_tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Evicts least recently used plans beyond `capacity`. A plan someone else still holds
    /// (a mutation in flight) or has locked is skipped, so the cache can exceed `capacity`
    /// until it is released.
    fn trim(&mut self, capacity: usize) {
        while self.entries.len() > capacity {
            let victim = self
                .entries
                .iter()
                .filter(|(_, slot)| Arc::strong_count(&slot.plan) == 1 && !slot.plan.is_locked())
                .min_by_key(|(_, slot)| slot.last_used)
                .map(|(id, _)| id.clone());
            match victim {
                Some(id) => {
                    self.entries.remove(&id);
                }
                None => return,
            }
        }
    }
}

impl PlanStore {
    /// The one handle by-id readers and writers lock: cached, else loaded from SQL.
    /// An id the index does not know is `NotFound` without touching SQL.
    pub(in crate::service_plan) fn plan_arc(&self, id: &str) -> Result<PlanHandle, PlanError> {
        #[cfg(any(test, feature = "test-support"))]
        self.plan_touches.lock().push(id.to_string());
        if let Some(handle) = self.plan_cache.get(id) {
            return Ok(handle);
        }
        if !self.index.read().contains_key(id) {
            return Err(PlanError::NotFound);
        }
        let epoch = self.plan_cache.epoch();
        let plan = self.load_plan_from_sql(id)?.ok_or(PlanError::NotFound)?;
        Ok(self.plan_cache.admit(id, Arc::new(Mutex::new(plan)), epoch))
    }

    /// Every plan load from SQL goes through here, so test builds can prove which plans a path loaded.
    pub(in crate::service_plan) fn load_plan_from_sql(
        &self,
        id: &str,
    ) -> Result<Option<Plan>, PlanError> {
        #[cfg(any(test, feature = "test-support"))]
        self.plan_loads.lock().push(id.to_string());
        self.database.load_plan(id)
    }

    /// A plan that cannot be loaded (`NotFound`, `Quarantined`) is reported, never cached.
    pub fn try_get(&self, id: &str) -> Result<Plan, PlanError> {
        Ok(self.plan_arc(id)?.lock().clone())
    }

    /// Ids of every plan loaded from SQL since the last reset, in load order.
    #[cfg(any(test, feature = "test-support"))]
    pub fn plan_loads_for_test(&self) -> Vec<String> {
        self.plan_loads.lock().clone()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn reset_plan_loads_for_test(&self) {
        self.plan_loads.lock().clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn cached_plan_ids_for_test(&self) -> Vec<String> {
        self.plan_cache.cached_ids()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_plan_cache_for_test(&self) {
        self.plan_cache.clear();
    }

    /// Forgets every loaded plan and the index, as if none had been read since a fresh start.
    #[cfg(test)]
    pub(in crate::service_plan) fn forget_all_plans_for_test(&self) {
        self.plan_cache.clear();
        self.index.write().clear();
    }

    /// The cached handle of a plan, loading it first; lets a test edit memory directly.
    #[cfg(test)]
    pub(in crate::service_plan) fn plan_arc_for_test(&self, id: &str) -> PlanHandle {
        self.plan_arc(id).expect("plan loads for test")
    }
}

#[cfg(test)]
mod tests;
