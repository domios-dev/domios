use super::PlanStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const PENDING_PLAN_LIMIT: usize = 128;
const CHANGED_TASK_LIMIT: usize = 128;
const CHANGED_TASK_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct CommittedPlanChange {
    pub plan_id: String,
    pub revision: String,
    pub previous_revision: String,
    pub task_ids: Vec<String>,
    pub reset: bool,
    pub deleted: bool,
}

pub enum PlanChangeNotice {
    Committed(CommittedPlanChange),
    Resync,
}

#[derive(Default)]
pub(super) struct PendingPlanChanges {
    plans: BTreeMap<String, CommittedPlanChange>,
    overflow: bool,
}

impl PendingPlanChanges {
    pub(super) fn insert(&mut self, mut change: CommittedPlanChange) {
        if self.overflow {
            return;
        }
        if let Some(previous) = self.plans.remove(&change.plan_id) {
            change.previous_revision = previous.previous_revision;
            change.reset |= previous.reset;
            let tasks: BTreeSet<_> = previous
                .task_ids
                .into_iter()
                .chain(change.task_ids)
                .collect();
            change.task_ids = tasks.into_iter().collect();
        } else if self.plans.len() == PENDING_PLAN_LIMIT {
            self.plans.clear();
            self.overflow = true;
            return;
        }
        if change.task_ids.len() > CHANGED_TASK_LIMIT
            || change.task_ids.iter().map(String::len).sum::<usize>() > CHANGED_TASK_BYTES
        {
            change.reset = true;
        }
        if change.reset {
            change.task_ids.clear();
        }
        self.plans.insert(change.plan_id.clone(), change);
    }

    pub(super) fn take(&mut self, plan_id: &str) -> Option<PlanChangeNotice> {
        if std::mem::take(&mut self.overflow) {
            self.plans.clear();
            Some(PlanChangeNotice::Resync)
        } else {
            self.plans.remove(plan_id).map(PlanChangeNotice::Committed)
        }
    }
}

impl PlanStore {
    pub fn take_committed_change(&self, plan_id: &str) -> Option<PlanChangeNotice> {
        self.database.pending_changes.lock().take(plan_id)
    }
}

#[cfg(test)]
mod tests;
