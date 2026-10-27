use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    FileTooLarge,
    FileDailyLimit,
    SenderQuota,
    FilesDisabled,
    FileRateLimited,
    FileNotFound,
    FileNotReady,
    UploadMismatch,
    RecipientFilesUnsupported,
    EntitlementsUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Failure {
    pub reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_files: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

impl Failure {
    pub fn too_large() -> Self {
        Self {
            reason: Reason::FileTooLarge,
            limit_bytes: Some((super::MAX_FILE_BYTES - super::FILE_OVERHEAD) as u64),
            limit_files: None,
            resets_at: None,
            retry_after_seconds: None,
        }
    }

    pub fn from_callable(body: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(body.pointer("/error/details")?.clone()).ok()
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = serde_json::to_value(self.reason).map_err(|_| std::fmt::Error)?;
        let details = serde_json::to_string(self).map_err(|_| std::fmt::Error)?;
        write!(f, "{}: {details}", reason.as_str().ok_or(std::fmt::Error)?)
    }
}
