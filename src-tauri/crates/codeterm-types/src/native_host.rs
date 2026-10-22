use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

pub const NATIVE_HOST_PROTOCOL: u32 = 3;
pub const MAX_NATIVE_HOST_FRAME_BYTES: usize = 32 * 1024;
pub const MAX_PENDING_NATIVE_HOST_REQUESTS: usize = 64;
pub const NATIVE_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(10);
pub const NATIVE_UPDATE_OBSERVATION_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// Hex length of the identity digest inside local socket names. Unix socket paths are
/// capped at 104 bytes on macOS, where namespaced names land under a long `$TMPDIR`.
pub const BOOTSTRAP_IDENTITY_HEX_LEN: usize = 16;

pub fn native_host_socket_address(bootstrap_address: &str) -> Option<String> {
    let identity = bootstrap_address
        .strip_prefix("domios-bootstrap-")?
        .strip_suffix(".sock")?;
    if identity.len() != BOOTSTRAP_IDENTITY_HEX_LEN
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    Some(format!("domios-native-host-{identity}.sock"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostConnection {
    pub instance_id: String,
    pub identity: crate::server_bootstrap::ServerIdentity,
    pub server_epoch: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeViewBinding {
    pub origin: String,
    pub session_id: Uuid,
}

impl NativeViewBinding {
    pub fn observed(origin: String) -> Result<Self, NativeHostRejection> {
        let binding = Self {
            origin,
            session_id: Uuid::new_v4(),
        };
        binding
            .is_well_formed()
            .then_some(binding)
            .ok_or(NativeHostRejection::InvalidView)
    }

    pub fn is_well_formed(&self) -> bool {
        let Some((scheme, authority)) = self.origin.split_once("://") else {
            return false;
        };
        let mut scheme_chars = scheme.chars();
        let Some(first) = scheme_chars.next() else {
            return false;
        };
        first.is_ascii_alphabetic()
            && scheme_chars
                .all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'))
            && !authority.is_empty()
            && authority.chars().all(|char| {
                !char.is_control()
                    && !char.is_whitespace()
                    && !matches!(char, '/' | '?' | '#' | '@')
            })
            && self.origin.len() <= 2048
            && !self.session_id.is_nil()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostChallenge {
    pub protocol: u32,
    pub challenge: String,
    pub connection: NativeHostConnection,
}

impl NativeHostChallenge {
    pub fn new(connection: NativeHostConnection) -> Self {
        Self {
            protocol: NATIVE_HOST_PROTOCOL,
            challenge: Uuid::new_v4().to_string(),
            connection,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostEnrollmentRequest {
    pub protocol: u32,
    pub challenge: String,
    pub connection: NativeHostConnection,
    pub view: Option<NativeViewBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostEnrollment {
    pub protocol: u32,
    pub challenge: String,
    pub connection: NativeHostConnection,
    pub client_process: crate::server_bootstrap::ProcessProvenance,
    pub view: Option<NativeViewBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "scope", content = "view_session", rename_all = "snake_case")]
pub enum NativeHostScope {
    Client,
    View(Uuid),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "payload", rename_all = "snake_case")]
pub enum NativeHostOperation<UpdateRequest> {
    ExactQuit {
        expected: Box<crate::app_quit::QuitSnapshot>,
    },
    FocusWindow {
        label: String,
    },
    WindowExists {
        label: String,
    },
    OsNotify {
        title: String,
        body: Option<String>,
    },
    ToastPermission {
        enabling: Option<bool>,
    },
    OpenWebview {
        request: NativeWebviewRequest,
    },
    CopyImageToClipboard {
        path: PathBuf,
    },
    NativeUpdate {
        request: UpdateRequest,
        apply: Option<crate::native_update::apply::NativeUpdateApplyOptions>,
    },
    ObserveNativeUpdate {
        operation_id: crate::native_update::NativeUpdateOperationId,
        after_sequence: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWebviewRequest {
    pub label: String,
    pub url: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", content = "payload", rename_all = "snake_case")]
pub enum NativeHostResult<UpdateResult> {
    Acknowledged,
    ExactQuitAdmitted,
    ExactQuitRefused(crate::app_quit::QuitRefusal),
    WindowExists(bool),
    ToastPermission(String),
    NativeUpdate(UpdateResult),
    NativeUpdateApply(crate::native_update::apply::NativeUpdateApplyResult),
    NativeUpdateObservation(crate::native_update::NativeUpdateObservationOutcome),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeHostRejection {
    NotAttached,
    QueueFull,
    RequestIdsExhausted,
    DeadlineBeforeDispatch,
    Unauthorized,
    InvalidView,
    Unsupported,
    ProtocolMismatch,
    HostRejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeHostUncertainty {
    CancelledAfterDispatch,
    DeadlineAfterDispatch,
    DisconnectedAfterDispatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "completion", content = "payload", rename_all = "snake_case")]
pub enum NativeHostCompletion<T> {
    Completed(T),
    Rejected(NativeHostRejection),
    Uncertain(NativeHostUncertainty),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeHostRequestId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostRequest<UpdateRequest> {
    pub request_id: NativeHostRequestId,
    pub connection: NativeHostConnection,
    pub scope: NativeHostScope,
    pub operation: NativeHostOperation<UpdateRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostReply<UpdateResult> {
    pub request_id: NativeHostRequestId,
    pub connection: NativeHostConnection,
    pub scope: NativeHostScope,
    pub completion: NativeHostCompletion<NativeHostResult<UpdateResult>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostCancellation {
    pub request_id: NativeHostRequestId,
    pub connection: NativeHostConnection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "audience", content = "target", rename_all = "snake_case")]
pub enum NativeHostEventAudience {
    Generic,
    Native,
    Targeted(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostEvent {
    pub event_id: u64,
    pub connection: NativeHostConnection,
    pub audience: NativeHostEventAudience,
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeWebviewDiagnosticsUnavailableReason {
    PortOccupied,
    EndpointNotAttributed,
    WebviewInitializationFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "payload", rename_all = "snake_case")]
pub enum NativeWebviewDiagnosticsStatus {
    Disabled,
    Unsupported,
    Unavailable {
        reason: NativeWebviewDiagnosticsUnavailableReason,
    },
    Active {
        port: u16,
        view_generation: u64,
    },
}

impl NativeWebviewDiagnosticsStatus {
    pub fn is_well_formed(&self) -> bool {
        match self {
            Self::Active {
                port,
                view_generation,
            } => (1024..=u16::MAX).contains(port) && *view_generation > 0,
            Self::Disabled | Self::Unsupported | Self::Unavailable { .. } => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWebviewDiagnosticsReport {
    pub connection: NativeHostConnection,
    pub view_session: Uuid,
    pub status: NativeWebviewDiagnosticsStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWebviewDiagnosticsSnapshot {
    pub connection: NativeHostConnection,
    pub view_session: Uuid,
    pub client_process: crate::server_bootstrap::ProcessProvenance,
    pub status: NativeWebviewDiagnosticsStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum NativeHostFrame<UpdateRequest, UpdateResult> {
    Challenge(NativeHostChallenge),
    Enroll(NativeHostEnrollmentRequest),
    Enrolled(NativeHostEnrollment),
    BindView {
        connection: NativeHostConnection,
        view: NativeViewBinding,
    },
    ReleaseView {
        connection: NativeHostConnection,
        session_id: Uuid,
    },
    Request(NativeHostRequest<UpdateRequest>),
    Reply(NativeHostReply<UpdateResult>),
    Cancel(NativeHostCancellation),
    DiagnosticsStatus(NativeWebviewDiagnosticsReport),
    Event(NativeHostEvent),
}

#[cfg(test)]
mod tests;
