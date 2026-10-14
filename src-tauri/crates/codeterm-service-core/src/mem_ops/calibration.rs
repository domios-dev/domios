use serde::Deserialize;

pub use codeterm_mem::curation::calibration::archive;
pub use codeterm_mem::curation::calibration::calibration_log_query;
pub use codeterm_mem::curation::calibration::run_calibration_sweep;
pub use codeterm_mem::curation::calibration::run_nightly_calibration;
pub use codeterm_mem::curation::calibration::stale;

#[derive(Deserialize)]
pub struct SweepParams {
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Deserialize)]
pub struct CalibLogParams {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub count: Option<bool>,
}
