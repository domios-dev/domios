use super::NativeUpdateOperationId;
use crate::{
    native_host::NativeHostConnection, server_bootstrap::ServerReady, update::channel::Channel,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateApplyOptions {
    pub channel: Channel,
    pub manifest_url: Option<String>,
    pub initiating_tab_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", content = "payload", rename_all = "snake_case")]
pub enum NativeUpdateApplyPhase {
    Admitted,
    Downloading,
    Applying,
    Relaunching,
    NoUpdate,
    Failed {
        message: String,
    },
    Verified {
        version: String,
        server: Box<ServerReady>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateApplyResult {
    pub operation_id: NativeUpdateOperationId,
    pub connection: NativeHostConnection,
    pub sequence: u64,
    pub phase: NativeUpdateApplyPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUpdateAdmissionRefusal {
    OperationConflict,
    RetiredOperation,
    IncarnationLimit,
    StaleServerIncarnation,
}

impl NativeUpdateAdmissionRefusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::OperationConflict => "No installer was started. This operation ID was already used with different update options. Check for updates and start a new update.",
            Self::RetiredOperation => "No installer was started. This operation was already admitted, and its recorded result is no longer retained. Check for updates and start a new update.",
            Self::IncarnationLimit => "No installer was started. This Domios server has reached its update admission limit. Restart Domios, then check for updates and update again.",
            Self::StaleServerIncarnation => "No installer was started. This request belongs to a previous Domios server. Check for updates and start a new update.",
        }
    }

    pub fn error(self) -> String {
        serde_json::json!({
            "kind": "native_update_admission_refused", "reason": self, "message": self.message(),
        })
        .to_string()
    }

    pub fn from_error(raw: &str) -> Option<Self> {
        let body: serde_json::Value = serde_json::from_str(raw).ok()?;
        if body["kind"] != "native_update_admission_refused" {
            return None;
        }
        serde_json::from_value(body["reason"].clone()).ok()
    }
}

pub const INSTALL_HANDOFF_FAILED_KIND: &str = "install_handoff_failed";
pub const RELAUNCH_RECOVERY: &str = "Relaunch the Domios desktop app and retry the update.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallFailureCode {
    ServerAttachmentUnavailable,
    NativeCapabilityUnavailable,
    UpdateAdmissionRefused,
    InstallFileHolders,
    DownloadOrSignature,
    ManifestFetch,
    ServerStopFailed,
    InstallerLaunchFailed,
    UpdateOutcomeUncertain,
    InstallHandoffFailed,
}

impl InstallFailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServerAttachmentUnavailable => "server_attachment_unavailable",
            Self::NativeCapabilityUnavailable => "native_capability_unavailable",
            Self::UpdateAdmissionRefused => "update_admission_refused",
            Self::InstallFileHolders => "install_file_holders",
            Self::DownloadOrSignature => "download_or_signature",
            Self::ManifestFetch => "manifest_fetch",
            Self::ServerStopFailed => "server_stop_failed",
            Self::InstallerLaunchFailed => "installer_launch_failed",
            Self::UpdateOutcomeUncertain => "update_outcome_uncertain",
            Self::InstallHandoffFailed => "install_handoff_failed",
        }
    }

    pub fn http_status(self) -> u16 {
        match self {
            Self::ServerAttachmentUnavailable
            | Self::NativeCapabilityUnavailable
            | Self::UpdateAdmissionRefused
            | Self::InstallFileHolders
            | Self::ServerStopFailed => 409,
            Self::DownloadOrSignature | Self::ManifestFetch | Self::UpdateOutcomeUncertain => 502,
            Self::InstallerLaunchFailed | Self::InstallHandoffFailed => 500,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallHandoffFailure {
    pub code: InstallFailureCode,
    pub message: String,
    pub recovery: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl InstallHandoffFailure {
    pub fn classify(raw: &str) -> Self {
        let trimmed = raw.trim();
        if let Some(existing) = Self::from_wire(trimmed) {
            return existing;
        }
        if let Some(refusal) = NativeUpdateAdmissionRefusal::from_error(trimmed) {
            let recovery = match refusal {
                NativeUpdateAdmissionRefusal::IncarnationLimit => {
                    "Relaunch Domios, then check for updates and retry."
                }
                _ => "Check for updates and start a new update.",
            };
            return Self {
                code: InstallFailureCode::UpdateAdmissionRefused,
                message: refusal.message().to_owned(),
                recovery: recovery.to_owned(),
                detail: None,
            };
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("server attachment is unavailable")
            || lower.contains("native host is unavailable")
            || lower.contains("native host attachment is unavailable")
            || lower.contains("native updater attachment is unavailable")
            || lower.contains("attached server process identity changed")
            || lower.contains("notattached")
        {
            return Self {
                code: InstallFailureCode::ServerAttachmentUnavailable,
                message: "The native host attachment to the Domios server is unavailable; the installer handoff was refused.".into(),
                recovery: RELAUNCH_RECOVERY.into(),
                detail: (!trimmed.is_empty()).then(|| trimmed.to_owned()),
            };
        }
        if lower.contains("native_capability_unavailable") {
            return Self {
                code: InstallFailureCode::NativeCapabilityUnavailable,
                message: "Automatic installation requires a native application host attached to this server.".into(),
                recovery: "Relaunch the Domios desktop app, or run domios update install --from-file <installer-path>.".into(),
                detail: None,
            };
        }
        if lower.contains("install-file holders") || trimmed.contains("[update-blocking-tabs]") {
            let summary = trimmed
                .split("\n[update-blocking-tabs]")
                .next()
                .unwrap_or(trimmed)
                .trim();
            return Self {
                code: InstallFailureCode::InstallFileHolders,
                message: summary.to_owned(),
                recovery: "Close or finish the blocking tabs and retry the update.".into(),
                detail: (summary != trimmed).then(|| trimmed.to_owned()),
            };
        }
        if lower.contains("download/verify") {
            return Self {
                code: InstallFailureCode::DownloadOrSignature,
                message: trimmed.to_owned(),
                recovery: "Check network connectivity and retry the update.".into(),
                detail: None,
            };
        }
        if lower.contains("manifest") || lower.contains("fetch release") {
            return Self {
                code: InstallFailureCode::ManifestFetch,
                message: trimmed.to_owned(),
                recovery: "Check network connectivity and retry the update.".into(),
                detail: None,
            };
        }
        if lower.contains("server did not stop")
            || (lower.contains("server") && lower.contains("stop"))
        {
            return Self {
                code: InstallFailureCode::ServerStopFailed,
                message: trimmed.to_owned(),
                recovery: RELAUNCH_RECOVERY.into(),
                detail: None,
            };
        }
        if lower.contains("installer launch") || lower.contains("client cleanup did not complete") {
            return Self {
                code: InstallFailureCode::InstallerLaunchFailed,
                message: trimmed.to_owned(),
                recovery: "Relaunch Domios and retry the update, or install from the official release package.".into(),
                detail: None,
            };
        }
        if lower.contains("outcome uncertain") || lower.contains("not be replayed") {
            return Self {
                code: InstallFailureCode::UpdateOutcomeUncertain,
                message: trimmed.to_owned(),
                recovery: "Check domios update status and relaunch Domios if the new version did not start.".into(),
                detail: None,
            };
        }
        Self {
            code: InstallFailureCode::InstallHandoffFailed,
            message: if trimmed.is_empty() {
                "Installer handoff failed.".into()
            } else {
                trimmed.to_owned()
            },
            recovery: RELAUNCH_RECOVERY.into(),
            detail: None,
        }
    }

    pub fn from_wire(raw: &str) -> Option<Self> {
        let body: serde_json::Value = serde_json::from_str(raw).ok()?;
        if body.get("kind").and_then(serde_json::Value::as_str) != Some(INSTALL_HANDOFF_FAILED_KIND)
        {
            return None;
        }
        serde_json::from_value(body).ok()
    }

    pub fn to_api_json(&self) -> serde_json::Value {
        let mut body = serde_json::json!({
            "ok": false,
            "kind": INSTALL_HANDOFF_FAILED_KIND,
            "code": self.code.as_str(),
            "message": self.message,
            "recovery": self.recovery,
            "hint": self.recovery,
        });
        if let Some(detail) = &self.detail {
            body["detail"] = serde_json::json!(detail);
        }
        body
    }

    pub fn to_wire_string(&self) -> String {
        self.to_api_json().to_string()
    }
}

#[cfg(test)]
mod tests;
