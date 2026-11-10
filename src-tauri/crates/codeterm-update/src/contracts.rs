use serde::Serialize;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct RuntimeStatus {
    pub phase: crate::UpdatePhase,
    pub last_error: Option<String>,
    pub error_revision: Option<String>,
    pub channel: crate::Channel,
    pub install_method: String,
    pub current_version: String,
    pub latest_known: Option<String>,
    pub update_available: bool,
    pub check_path: Option<String>,
    pub phase_age_secs: Option<u64>,
    pub downloaded_bytes: u64,
    pub download_total_bytes: Option<u64>,
    pub download_progress: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Updating,
    #[default]
    UpToDate,
    Command,
    BrowserOpened,
    AlreadyInFlight,
    Scheduled,
}

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
pub struct InstallResponse {
    pub status: InstallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub url: Option<String>,
}

#[cfg(test)]
mod tests;
