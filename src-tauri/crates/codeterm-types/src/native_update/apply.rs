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
