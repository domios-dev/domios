use serde::Deserialize;

pub(crate) use codeterm_mem::curation::calibration::{
    archive, calibration_log_query, run_calibration_sweep, run_nightly_calibration, stale,
};

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
