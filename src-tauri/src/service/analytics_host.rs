//! The provider-neutral KPI analytics read behind `host.analytics`. Buckets follow
//! the caller's time zone, the ledger answers from its generation-cached aggregate,
//! and the plan crate folds that into the snapshot. No ledger handle or path leaves.

use chrono::{Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;
use codeterm_plan::service_plan::state::analytics::{
    local_buckets, project_snapshot, validate_query, AnalyticsQuery, AnalyticsRefusal,
    AnalyticsSnapshot, BucketBound, DirectoryPlan, LedgerRead, ProjectionInput, ScopeDirectory,
};

use super::ServiceContext;

impl ServiceContext {
    /// JSON in, JSON out: the snapshot, or `{error, message}` for a refused query.
    pub fn analytics_snapshot_json(&self, query_json: &str) -> String {
        let encoded = match self.analytics_snapshot(query_json) {
            Ok(snapshot) => serde_json::to_string(&snapshot),
            Err(refusal) => serde_json::to_string(&refusal),
        };
        encoded.unwrap_or_else(|e| {
            serde_json::json!({ "error": "internal", "message": e.to_string() }).to_string()
        })
    }

    pub(crate) fn analytics_snapshot(
        &self,
        query_json: &str,
    ) -> Result<AnalyticsSnapshot, AnalyticsRefusal> {
        let query: AnalyticsQuery = serde_json::from_str(query_json)
            .map_err(|e| AnalyticsRefusal::invalid(format!("malformed analytics query: {e}")))?;
        let days = validate_query(&query)?;
        let tz: Tz = query.range.tz.parse().map_err(|_| {
            AnalyticsRefusal::invalid(format!("unknown time zone: {}", query.range.tz))
        })?;
        let buckets = local_buckets(days, query.bucket)
            .iter()
            .map(|bucket| {
                Ok(BucketBound {
                    start: local_midnight(bucket.first_day, tz)?,
                    end: local_midnight(bucket.end_day, tz)?,
                    label: bucket.first_day.format("%Y-%m-%d").to_string(),
                })
            })
            .collect::<Result<Vec<_>, AnalyticsRefusal>>()?;
        let directory = self.analytics_directory();
        let ledger = self.analytics_ledger_read(&buckets);
        let allowance: Vec<_> = super::plan::mission_control_ext::provider_allowances(self)
            .into_iter()
            .collect();
        project_snapshot(&ProjectionInput {
            query: &query,
            buckets: &buckets,
            directory: &directory,
            ledger: &ledger,
            allowance: &allowance,
            now: crate::agent::kpi::attribution::now_epoch_secs(),
        })
    }

    fn analytics_directory(&self) -> ScopeDirectory {
        let mut directory = ScopeDirectory::default();
        {
            let mux = self.mux.read();
            for workspace in mux.list_workspaces() {
                directory.workspaces.insert(workspace.id, workspace.name);
            }
            for team in mux.list_teams() {
                directory.teams.insert(team.id, team.name);
            }
        }
        for plan in self.plan_store.list_index() {
            directory.plans.insert(
                plan.plan_id,
                DirectoryPlan {
                    title: plan.title,
                    team_id: plan.team_id,
                },
            );
        }
        for provider in self.agent_registry.provider_ids() {
            let name = self.agent_registry.with_provider(&provider, |registered| {
                registered.display_name().to_string()
            });
            if let Some(name) = name {
                directory.providers.insert(provider, name);
            }
        }
        directory
    }

    fn analytics_ledger_read(&self, buckets: &[BucketBound]) -> LedgerRead {
        let Some(ledger) = self.kpi.ledger() else {
            return match self.kpi.open_error() {
                Some(reason) => LedgerRead::Unavailable {
                    reason: reason.to_string(),
                },
                None => LedgerRead::Disabled,
            };
        };
        match ledger.analytics_aggregates(buckets) {
            Ok(aggregates) => LedgerRead::Recorded(aggregates),
            Err(reason) => LedgerRead::Unavailable { reason },
        }
    }
}

/// A zone whose clocks skip midnight starts that day at the first hour that exists.
fn local_midnight(day: NaiveDate, tz: Tz) -> Result<u64, AnalyticsRefusal> {
    let midnight = day
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid for a date");
    let instant = tz
        .from_local_datetime(&midnight)
        .earliest()
        .or_else(|| {
            tz.from_local_datetime(&(midnight + Duration::hours(1)))
                .earliest()
        })
        .ok_or_else(|| {
            AnalyticsRefusal::invalid(format!("{day} has no local midnight in {}", tz.name()))
        })?;
    u64::try_from(instant.timestamp())
        .map_err(|_| AnalyticsRefusal::invalid("range must not precede 1970-01-01"))
}

#[cfg(test)]
mod tests;
