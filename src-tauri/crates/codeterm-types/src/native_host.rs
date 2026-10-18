use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

pub const NATIVE_HOST_PROTOCOL: u32 = 2;
pub const MAX_NATIVE_HOST_FRAME_BYTES: usize = 32 * 1024;
pub const MAX_PENDING_NATIVE_HOST_REQUESTS: usize = 64;
pub const NATIVE_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(10);

pub fn native_host_socket_address(bootstrap_address: &str) -> Option<String> {
    let identity = bootstrap_address
        .strip_prefix("domios-bootstrap-")?
        .strip_suffix(".sock")?;
    if identity.len() != 64
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
    DebianPreflight,
    NativeUpdate {
        request: UpdateRequest,
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
    WindowExists(bool),
    ToastPermission(String),
    DebianPreflight(NativeDebianPreflight),
    NativeUpdate(UpdateResult),
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
        let observation = NativeHostOperation::<String>::ObserveNativeUpdate {
            operation_id: crate::native_update::NativeUpdateOperationId::new(Uuid::new_v4())
                .unwrap(),
            after_sequence: 8,
        };
        let value = serde_json::to_value(observation).unwrap();
        assert_eq!(value["operation"], "observe_native_update");
        assert_eq!(value["payload"]["after_sequence"], 8);
        assert!(
            serde_json::from_value::<NativeHostOperation<String>>(json!({
                "operation": "invoke",
                "payload": { "name": "focus_window" }
            }))
            .is_err()
        );
    }

    #[test]
    fn observation_outcomes_round_trip_through_the_native_host_frame() {
        let connection = NativeHostConnection {
            instance_id: "instance".into(),
            identity: crate::server_bootstrap::ServerIdentity {
                data: "data".into(),
                config: "config".into(),
            },
            server_epoch: "epoch".into(),
            generation: 3,
        };
        let update = crate::native_update::NativeUpdateResult {
            operation_id: crate::native_update::NativeUpdateOperationId::new(Uuid::new_v4())
                .unwrap(),
            sequence: 4,
            connection: connection.clone(),
            state: crate::native_update::NativeUpdateState::Uncertain {
                reason: crate::native_update::NativeUpdateUncertainty::DisconnectedAfterDispatch,
            },
        };
        let outcomes = [
            crate::native_update::NativeUpdateObservationOutcome::Reply(update),
            crate::native_update::NativeUpdateObservationOutcome::NoNewEvidence,
            crate::native_update::NativeUpdateObservationOutcome::Unavailable {
                reason: crate::native_update::NativeUpdateUncertainty::DeadlineAfterDispatch,
            },
        ];

        for outcome in outcomes {
            let frame = NativeHostFrame::<String, crate::native_update::NativeUpdateResult>::Reply(
                NativeHostReply {
                    request_id: NativeHostRequestId(17),
                    connection: connection.clone(),
                    scope: NativeHostScope::Client,
                    completion: NativeHostCompletion::Completed(
                        NativeHostResult::NativeUpdateObservation(outcome),
                    ),
                },
            );
            let encoded = serde_json::to_vec(&frame).unwrap();
            let decoded = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(decoded, frame);
        }
    }

    #[test]
    fn diagnostics_status_frame_carries_only_the_bound_client_view_claim() {
        let report = NativeWebviewDiagnosticsReport {
            connection: NativeHostConnection {
                instance_id: "instance".into(),
                identity: crate::server_bootstrap::ServerIdentity {
                    data: "data".into(),
                    config: "config".into(),
                },
                server_epoch: "epoch".into(),
                generation: 3,
            },
            view_session: Uuid::new_v4(),
            status: NativeWebviewDiagnosticsStatus::Active {
                port: 9222,
                view_generation: 4,
            },
        };
        let frame = NativeHostFrame::<String, String>::DiagnosticsStatus(report);
        let value = serde_json::to_value(&frame).unwrap();

        assert_eq!(value["kind"], "diagnostics_status");
        assert_eq!(value["payload"]["status"]["status"], "active");
        assert_eq!(value["payload"]["status"]["payload"]["port"], 9222);
        assert!(value["payload"].get("client_process").is_none());
        assert!(value["payload"].get("origin").is_none());
        assert_eq!(
            serde_json::from_value::<NativeHostFrame<String, String>>(value).unwrap(),
            frame
        );
    }

    #[test]
    fn diagnostics_active_status_rejects_reserved_ports_and_zero_view_generation() {
        assert!(!NativeWebviewDiagnosticsStatus::Active {
            port: 1023,
            view_generation: 1,
        }
        .is_well_formed());
        assert!(!NativeWebviewDiagnosticsStatus::Active {
            port: 9222,
            view_generation: 0,
        }
        .is_well_formed());
        assert!(NativeWebviewDiagnosticsStatus::Active {
            port: u16::MAX,
            view_generation: 1,
        }
        .is_well_formed());
    }

    #[test]
    fn native_event_keeps_its_audience_explicit() {
        let event = NativeHostEvent {
            event_id: 7,
            connection: NativeHostConnection {
                instance_id: "instance".into(),
                identity: crate::server_bootstrap::ServerIdentity {
                    data: "data".into(),
                    config: "config".into(),
                },
                server_epoch: "epoch".into(),
                generation: 3,
            },
            audience: NativeHostEventAudience::Targeted("main".into()),
            name: "selection-changed".into(),
            payload: json!({ "selection": "tab" }),
        };
        let value = serde_json::to_value(&event).unwrap();

        assert_eq!(
            value["audience"],
            json!({ "audience": "targeted", "target": "main" })
        );
        assert_eq!(value["payload"], json!({ "selection": "tab" }));
        assert_eq!(
            serde_json::from_value::<NativeHostEvent>(value).unwrap(),
            event
        );
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
            "scope": { "scope": "client" },
            "operation": {
                "operation": "window_exists",
                "payload": { "label": "main" }
            },
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

    #[test]
    fn native_host_socket_address_reuses_the_exact_bootstrap_identity() {
        let identity = "a".repeat(64);
        assert_eq!(
            native_host_socket_address(&format!("domios-bootstrap-{identity}.sock")),
            Some(format!("domios-native-host-{identity}.sock"))
        );
        assert_eq!(
            native_host_socket_address("domios-bootstrap-not-a-digest.sock"),
            None
        );
    }
}
