#[derive(Default, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExecResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub truncated: Option<bool>,
}

#[derive(serde::Serialize, ts_rs::TS)]
pub(super) struct ExecPollResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub done: Option<bool>,
    #[serde(flatten)]
    pub result: ExecResult,
}

pub(super) fn export(config: &ts_rs::Config) -> Result<(), Box<dyn std::error::Error>> {
    use ts_rs::TS;
    ExecResult::export_all(config)?;
    ExecPollResult::export_all(config)?;
    Ok(())
}

#[cfg(test)]
mod tests;
