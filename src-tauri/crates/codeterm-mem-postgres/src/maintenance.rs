use crate::PostgresMemStore;
use codeterm_mem::backend::WriteContext;
mod calibration;
mod recluster;
mod stale;

impl PostgresMemStore {
    pub(crate) fn maintenance_recluster_prompt(&self) -> Result<Option<String>, String> {
        self.transaction(recluster::prompt)
    }
    pub(crate) fn maintenance_recluster_finalize(
        &self,
        response: &str,
        model: &str,
        plan_dir: &std::path::Path,
    ) -> Result<codeterm_mem::dreaming::recluster::ReclusterReport, String> {
        recluster::finalize(self, response, model, plan_dir)
    }
    pub(crate) fn maintenance_calibration_sweep(
        &self,
        ctx: &WriteContext,
        force: bool,
    ) -> Result<serde_json::Value, String> {
        self.transaction(|tx| calibration::sweep(tx, ctx, force))
    }
    pub(crate) fn maintenance_calibration_log(
        &self,
        since: &str,
        limit: usize,
        count_only: bool,
    ) -> Result<serde_json::Value, String> {
        self.transaction(|tx| calibration::log(tx, since, limit, count_only))
    }
    pub(crate) fn maintenance_nightly_calibration(
        &self,
        ctx: &WriteContext,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, String> {
        self.transaction(|tx| {
            let mut report = calibration::sweep(tx, ctx, false)?;
            report["stale"] = stale::run(tx, hygiene, now, false, None)?;
            Ok(report)
        })
    }
    pub(crate) fn maintenance_stale(
        &self,
        _ctx: &WriteContext,
        p: &codeterm_mem::curation::calibration::stale::StaleParams,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
    ) -> Result<serde_json::Value, String> {
        self.transaction(|tx| {
            stale::run(
                tx,
                hygiene,
                chrono::Utc::now(),
                p.dry_run.unwrap_or(false),
                p.limit,
            )
        })
    }
}
