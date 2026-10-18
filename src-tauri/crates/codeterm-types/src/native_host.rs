use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

pub const NATIVE_HOST_PROTOCOL: u32 = 1;
pub const MAX_NATIVE_HOST_FRAME_BYTES: usize = 32 * 1024;
pub const MAX_PENDING_NATIVE_HOST_REQUESTS: usize = 64;
pub const NATIVE_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(10);

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "payload", rename_all = "snake_case")]
pub enum NativeHostOperation<UpdateRequest> {
    Restart,
    Exit { code: i32 },
    CopyImageToClipboard { path: PathBuf },
    DebianPreflight,
    NativeUpdate { request: UpdateRequest },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeDebianElevationStatus {
    Ready,
    ElevationRequired,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDebianPreflight {
    pub status: NativeDebianElevationStatus,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", content = "payload", rename_all = "snake_case")]
pub enum NativeHostResult<UpdateResult> {
    Acknowledged,
    DebianPreflight(NativeDebianPreflight),
    NativeUpdateAccepted(UpdateResult),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeHostRejection {
    NotAttached,
    QueueFull,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum NativeHostFrame<UpdateRequest, UpdateResult> {
    Challenge(NativeHostChallenge),
    Enroll(NativeHostEnrollmentRequest),
    Enrolled(NativeHostEnrollment),
    BindView(NativeViewBinding),
    ReleaseView { session_id: Uuid },
    Request(NativeHostRequest<UpdateRequest>),
    Reply(NativeHostReply<UpdateResult>),
    Cancel(NativeHostCancellation),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enrollment_does_not_accept_peer_process_claims() {
        let request = json!({
            "protocol": NATIVE_HOST_PROTOCOL,
            "challenge": "challenge",
            "connection": {
                "instance_id": "instance",
                "identity": { "data": "data", "config": "config" },
                "server_epoch": "epoch",
                "generation": 1
            },
            "view": null,
            "client_process": {
                "pid": 12,
                "created": "token",
                "executable": "client.exe"
            }
        });

        assert!(serde_json::from_value::<NativeHostEnrollmentRequest>(request).is_err());
    }

    #[test]
    fn native_operation_is_a_finite_typed_enum() {
        let operation = NativeHostOperation::<String>::NativeUpdate {
            request: "transaction".into(),
        };
        let value = serde_json::to_value(&operation).unwrap();

        assert_eq!(value["operation"], "native_update");
        assert_eq!(value["payload"]["request"], "transaction");
        assert!(serde_json::from_value::<NativeHostOperation<String>>(json!({
            "operation": "invoke",
            "payload": { "name": "restart" }
        }))
        .is_err());
    }

    #[test]
    fn native_request_does_not_accept_caller_origin_claims() {
        let request = json!({
            "request_id": 1,
            "connection": {
                "instance_id": "instance",
                "identity": { "data": "data", "config": "config" },
                "server_epoch": "epoch",
                "generation": 1
            },
            "scope": "client",
            "operation": { "operation": "restart" },
            "origin": "https://untrusted.example"
        });

        assert!(serde_json::from_value::<NativeHostRequest<String>>(request).is_err());
    }

    #[test]
    fn dispatched_disconnect_is_uncertain_not_rollback() {
        let completion: NativeHostCompletion<()> =
            NativeHostCompletion::Uncertain(NativeHostUncertainty::DisconnectedAfterDispatch);
        let value = serde_json::to_value(completion).unwrap();

        assert_eq!(value["completion"], "uncertain");
        assert_eq!(value["payload"], "disconnected_after_dispatch");
        assert!(value.get("rolled_back").is_none());
    }

    #[test]
    fn view_binding_requires_an_origin_and_non_nil_session() {
        let valid = NativeViewBinding::observed("tauri://localhost".into()).unwrap();
        let invalid = NativeViewBinding {
            origin: String::new(),
            session_id: Uuid::nil(),
        };

        assert!(valid.is_well_formed());
        assert!(!invalid.is_well_formed());
    }
}
