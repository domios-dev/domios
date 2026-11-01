use super::{Plan, PlanStore};

pub const PLAN_RETENTION_MS: u64 = 365 * DAY_MS;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunedPlan {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
}

impl PlanStore {
    /// Claims the UTC day of `now_ms` for retention; true at most once per day.
    pub fn claim_retention_day(&self, now_ms: u64) -> bool {
        let day = now_ms / DAY_MS;
        let mut last_day = self.retention_day.lock();
        if *last_day == Some(day) {
            return false;
        }
        *last_day = Some(day);
        true
    }

    /// Deletes every plan untouched for a year, with its spec, rows, search rows and index units.
    pub fn prune_stale_plans(&self, now_ms: u64) -> Vec<PrunedPlan> {
        let cutoff = now_ms.saturating_sub(PLAN_RETENTION_MS);
        let candidates: Vec<String> = self
            .index
            .read()
            .values()
            .filter(|row| is_stale(row.updated_at, cutoff))
            .map(|row| row.id.clone())
            .collect();
        let mut pruned = Vec::new();
        for id in candidates {
            let plan = match self.delete_if_untouched_since(&id, cutoff) {
                Ok(Some(plan)) => plan,
                Ok(None) => continue,
                Err(error) => {
                    log::warn!("[plan.retention] could not delete plan {id}: {error}");
                    continue;
                }
            };
            log::info!(
                "[plan.retention] deleted plan {} \"{}\" last updated {}",
                plan.id,
                plan.title,
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(plan.updated_at as i64)
                    .map(|time| time.to_rfc3339())
                    .unwrap_or_else(|| plan.updated_at.to_string())
            );
            pruned.push(PrunedPlan {
                id: plan.id,
                title: plan.title,
                updated_at: plan.updated_at,
            });
        }
        pruned
    }

    /// Rechecks staleness inside the writer turn, so a plan touched after the candidate scan survives.
    pub(super) fn delete_if_untouched_since(
        &self,
        id: &str,
        cutoff: u64,
    ) -> std::io::Result<Option<Plan>> {
        let writer_turn = self.mutation_writer.enter();
        let Some(plan) = self
            .get(id)
            .filter(|plan| is_stale(plan.updated_at, cutoff))
        else {
            return Ok(None);
        };
        self.delete_in_writer(&writer_turn, id, Some(plan.clone()))?;
        Ok(Some(plan))
    }
}

fn is_stale(updated_at: u64, cutoff: u64) -> bool {
    updated_at < cutoff
}

#[cfg(test)]
mod tests;
