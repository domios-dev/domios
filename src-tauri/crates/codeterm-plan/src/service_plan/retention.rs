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
    /// Runs [`Self::prune_stale_plans`] at most once per UTC day of `now_ms`.
    pub fn run_retention_if_due(&self, now_ms: u64) -> Vec<PrunedPlan> {
        let day = now_ms / DAY_MS;
        {
            let mut last_day = self.retention_day.lock();
            if *last_day == Some(day) {
                return Vec::new();
            }
            *last_day = Some(day);
        }
        self.prune_stale_plans(now_ms)
    }

    /// Deletes every plan untouched for a year, with its spec, rows, search rows and index units.
    pub fn prune_stale_plans(&self, now_ms: u64) -> Vec<PrunedPlan> {
        let cutoff = now_ms.saturating_sub(PLAN_RETENTION_MS);
        let stale: Vec<Plan> = self
            .plans
            .read()
            .values()
            .filter_map(|plan| {
                let plan = plan.lock();
                (plan.updated_at < cutoff).then(|| plan.clone())
            })
            .collect();
        let mut pruned = Vec::new();
        for plan in stale {
            let owned_specs = self.owned_specs(&plan.id);
            if let Err(error) = self.delete(&plan.id) {
                log::warn!(
                    "[plan.retention] could not delete plan {}: {}",
                    plan.id,
                    error
                );
                continue;
            }
            for spec in owned_specs {
                super::search_index::remove_spec_from_index(spec);
            }
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

    fn owned_specs(&self, plan_id: &str) -> Vec<super::Spec> {
        self.specs
            .read()
            .values()
            .map(|spec| spec.lock().clone())
            .filter(|spec| spec.plan_id.as_deref() == Some(plan_id))
            .collect()
    }
}

#[cfg(test)]
mod tests;
