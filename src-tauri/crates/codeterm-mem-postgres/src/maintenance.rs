use crate::PostgresMemStore;
use codeterm_mem::backend::WriteContext;

impl PostgresMemStore {
    pub(crate) fn maintenance_recluster_prompt(&self) -> Result<Option<String>, String> {
        Err(crate::unsupported("recluster_prompt"))
    }
    pub(crate) fn maintenance_recluster_finalize(
        &self,
        _response: &str,
        _model: &str,
        _plan_dir: &std::path::Path,
    ) -> Result<codeterm_mem::dreaming::recluster::ReclusterReport, String> {
        Err(crate::unsupported("recluster_finalize"))
    }
    pub(crate) fn maintenance_calibration_sweep(
        &self,
        _ctx: &WriteContext,
        _force: bool,
    ) -> Result<serde_json::Value, String> {
        Err(crate::unsupported("calibration_sweep"))
    }
    pub(crate) fn maintenance_calibration_log(
        &self,
        _since: &str,
        _limit: usize,
        _count_only: bool,
    ) -> Result<serde_json::Value, String> {
        Err(crate::unsupported("calibration_log"))
    }
    pub(crate) fn maintenance_nightly_calibration(
        &self,
        _ctx: &WriteContext,
        _hygiene: &codeterm_types::config::DreamHygieneConfig,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, String> {
        Err(crate::unsupported("nightly_calibration"))
    }
    pub(crate) fn maintenance_stale(
        &self,
        _ctx: &WriteContext,
        _p: &codeterm_mem::curation::calibration::stale::StaleParams,
        _hygiene: &codeterm_types::config::DreamHygieneConfig,
    ) -> Result<serde_json::Value, String> {
        Err(crate::unsupported("stale"))
    }
}
