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
