use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use codeterm_mesh::{
    account_mesh::BoxFuture,
    cloud_ingress::{connect_enrolled, CloudIngressHandle, CloudRoute, IngressError},
    cloud_outbox::{
        is_allowed_plan_rpc_method, CallerContext, CloudOutboxPort, InstallationPair,
        OutboundOperation, OutboundPeerRequest, OutboxError, OutboxId, OutboxRecordStatus,
        StreamDeliveryOutcome, UnconfirmedReason, DELIVERY_UNCONFIRMED, OUTBOX_FULL,
        PAYLOAD_TOO_LARGE, REFUSED_BEFORE_WRITE, WORKER_ACK_RPC_METHOD, WORKER_REPORT_RPC_METHOD,
    },
    transport::TransportError,
};
use codeterm_service_core::mesh::{
    crown::{AuthenticatedAgent, AuthenticatedOrigin},
    types::{RelayCommand, RelayResponse},
    AnnounceReport,
};

use super::{
    origin,
    peer_resolution::{
        peer_unroutable_error, resolve_exact_relay_target_route, resolve_relay_target_route,
        RelayTargetRoute,
    },
    relay_failure,
};
use crate::routes::AppState;

/// Typed refusal for cloud-originated relayed notifications pending the joint
/// security review on relayed notification authority.
pub const CLOUD_NOTIFICATION_RELAY_REFUSED: &str = "cloud_notification_relay_refused";

/// Typed refusal when an inbound-only cloud peer tries to relay an operation
/// outside Courier's closed outbox contract (never falls back to direct HTTP).
pub const CLOUD_OUTBOUND_OPERATION_UNSUPPORTED: &str = "cloud_outbound_operation_unsupported";

/// Failure bound when waiting on an attached reverse stream to settle an
/// enqueued outbox record (covers write + ack timeouts without polling).
const CLOUD_OUTBOX_WAIT_DEADLINE: Duration = Duration::from_secs(95);

#[derive(Debug)]
pub enum CloudIngressSendError {
    ConnectUnavailable(String),
    AdmissionRejected(String),
    Transport(TransportError),
}

impl CloudIngressSendError {
    pub fn from_ingress(error: IngressError) -> Self {
        match error {
            IngressError::Rejected => {
                Self::AdmissionRejected("cloud ingress admission rejected".into())
            }
            IngressError::Unavailable => {
                Self::ConnectUnavailable("cloud ingress connect unavailable".into())
            }
            IngressError::InvalidFrame => {
                Self::ConnectUnavailable("cloud ingress handshake frame invalid".into())
            }
            IngressError::TlsFailed => {
                Self::AdmissionRejected("cloud ingress TLS verification failed".into())
            }
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::ConnectUnavailable(_) => StatusCode::BAD_GATEWAY.as_u16(),
            Self::AdmissionRejected(_) => StatusCode::FORBIDDEN.as_u16(),
            Self::Transport(error) => error.http_status(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::ConnectUnavailable(_) => {
                codeterm_mesh::contract::delivery::MESH_TRANSPORT_NOTHING_SENT
            }
            Self::AdmissionRejected(_) => {
                codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED
            }
            Self::Transport(error) => error.code(),
        }
    }

    pub fn bytes_written(&self) -> Option<bool> {
        match self {
            Self::ConnectUnavailable(_) | Self::AdmissionRejected(_) => Some(false),
            Self::Transport(error) => error.bytes_written(),
        }
    }
}

impl std::fmt::Display for CloudIngressSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectUnavailable(detail) | Self::AdmissionRejected(detail) => {
                write!(f, "{}: {detail}", self.code())
            }
            Self::Transport(error) => std::fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for CloudIngressSendError {}

pub trait CloudIngressConnector: Send + Sync {
    fn send_relay(
        &self,
        mesh: codeterm_mesh::MeshState,
        route: CloudRoute,
        installation_id: String,
        command: RelayCommand,
        signed_token: String,
    ) -> BoxFuture<'_, Result<RelayResponse, CloudIngressSendError>>;
}

struct LiveCloudIngressConnector;

impl CloudIngressConnector for LiveCloudIngressConnector {
    fn send_relay(
        &self,
        mesh: codeterm_mesh::MeshState,
        route: CloudRoute,
        installation_id: String,
        command: RelayCommand,
        signed_token: String,
    ) -> BoxFuture<'_, Result<RelayResponse, CloudIngressSendError>> {
        Box::pin(async move {
            let channel = connect_enrolled(mesh, route, &installation_id)
                .await
                .map_err(CloudIngressSendError::from_ingress)?;
            channel
                .send_relay(command, signed_token)
                .await
                .map_err(CloudIngressSendError::Transport)
        })
    }
}

/// Optional host extension allowing registered-router tests to substitute a
/// fake enrolled cloud channel without bypassing any relay guard.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub struct CloudIngressConnectorOverride(pub Arc<dyn CloudIngressConnector>);

/// Optional host extension allowing registered-router tests to bind a test
/// `CloudOutboxPort` on `ServiceContext` without opening a live ingress socket.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub struct CloudOutboxOverride(pub Arc<dyn CloudOutboxPort>);

pub(crate) async fn send_via_cloud_ingress(
    state: &AppState,
    route: CloudRoute,
    installation_id: &str,
    command: RelayCommand,
    signed_token: String,
) -> Result<RelayResponse, CloudIngressSendError> {
    let result = {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(connector) = state
            .svc
            .try_ext::<CloudIngressConnectorOverride>()
            .map(|ext| Arc::clone(&ext.0))
        {
            connector
                .send_relay(
                    state.mesh.clone(),
                    route,
                    installation_id.to_owned(),
                    command,
                    signed_token,
                )
                .await
        } else {
            LiveCloudIngressConnector
                .send_relay(
                    state.mesh.clone(),
                    route,
                    installation_id.to_owned(),
                    command,
                    signed_token,
                )
                .await
        }
        #[cfg(not(any(test, feature = "test-support")))]
        LiveCloudIngressConnector
            .send_relay(
                state.mesh.clone(),
                route,
                installation_id.to_owned(),
                command,
                signed_token,
            )
            .await
    };
    let online_changed = match &result {
        Ok(_) => state
            .mesh
            .lock()
            .record_cloud_relay_outcome(installation_id, true),
        Err(CloudIngressSendError::ConnectUnavailable(_)) => state
            .mesh
            .lock()
            .record_cloud_relay_outcome(installation_id, false),
        Err(_) => false,
    };
    if online_changed {
        state
            .svc
            .ui
            .emit("mesh-state-changed", serde_json::Value::Null);
    }
    result
}

/// Resolves an enrolled cloud peer's ingress endpoint. For managed cloud
/// machines the controller's backend-provisioned `endpoint` is authoritative
/// over gossip; unmanaged cloud peers use the verified gossip endpoint.
pub(crate) fn resolve_cloud_ingress_endpoint(
    state: &AppState,
    peer_id: &str,
    installation_id: &str,
    known: Option<CloudRoute>,
) -> Option<CloudRoute> {
    let machines = state.svc.cloud_machines();
    if let Some(record) = machines
        .find_by_peer(peer_id)
        .or_else(|| machines.find_by_peer(installation_id))
    {
        return record.endpoint;
    }
    known
}

pub(crate) fn has_cloud_outbox(state: &AppState) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if state.svc.try_ext::<CloudOutboxOverride>().is_some() {
        return true;
    }
    state.svc.try_ext::<CloudIngressHandle>().is_some()
}

async fn resolve_cloud_outbox(
    state: &AppState,
) -> Result<Arc<dyn CloudOutboxPort>, (StatusCode, String)> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(ext) = state.svc.try_ext::<CloudOutboxOverride>() {
        return Ok(Arc::clone(&ext.0));
    }
    let handle = state.svc.try_ext::<CloudIngressHandle>().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "cloud outbox unavailable: ingress handle not installed".into(),
    ))?;
    let outbox = handle.outbox().await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("cloud outbox unavailable: {error:?}"),
        )
    })?;
    Ok(outbox as Arc<dyn CloudOutboxPort>)
}

pub(super) enum OutboundRelayTransport {
    Http(HttpRelay),
    CloudIngress {
        installation_id: String,
        endpoint: Option<CloudRoute>,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
    },
    CloudOutbox {
        target: CloudOutboxTarget,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
    },
}

pub(super) struct HttpRelay {
    pub(super) primary: String,
    pub(super) alternatives: Vec<String>,
    pub(super) origin: AuthenticatedOrigin,
    pub(super) agent: Option<AuthenticatedAgent>,
}

impl HttpRelay {
    pub(super) fn client(
        &self,
        state: &AppState,
    ) -> Result<codeterm_mesh::transport::PeerClient, (StatusCode, String)> {
        let token = super::sign_caller(state, self.origin, self.agent.as_ref())?;
        Ok(codeterm_mesh::transport::PeerClient::new_signed_route(
            self.primary.clone(),
            self.alternatives.clone(),
            &token,
        ))
    }
}

pub(super) fn resolve_outbound_transport(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    target: &(Option<String>, Option<String>),
    inbound_only: bool,
) -> Result<
    (
        AuthenticatedOrigin,
        OutboundRelayTransport,
        String,
        String,
        Option<String>,
    ),
    (StatusCode, String),
> {
    let cloud_machines = state.svc.cloud_machines();
    let (route, peer_id, peer_name, peer_version) = {
        let mesh = state.mesh.lock();
        match target {
            (Some(id), None) => {
                resolve_exact_relay_target_route(&mesh, &cloud_machines, inbound_only, id)?
            }
            (None, Some(target)) => {
                resolve_relay_target_route(&mesh, &cloud_machines, inbound_only, target)?
            }
            _ => return Err((StatusCode::BAD_REQUEST, "Provide one relay target".into())),
        }
    };
    let (request_origin, transport) = match route {
        RelayTargetRoute::Http {
            primary,
            alternatives,
        } => {
            let (origin, agent) = origin::authenticated_local_caller(state, headers)?;
            (
                origin,
                OutboundRelayTransport::Http(HttpRelay {
                    primary,
                    alternatives,
                    origin,
                    agent,
                }),
            )
        }
        RelayTargetRoute::CloudIngress {
            installation_id,
            endpoint,
        } => {
            let (origin, agent) = origin::authenticated_local_caller(state, headers)?;
            (
                origin,
                OutboundRelayTransport::CloudIngress {
                    installation_id,
                    endpoint,
                    origin,
                    agent,
                },
            )
        }
        RelayTargetRoute::CloudOutbox {
            account_id,
            source_installation_id,
            target_installation_id,
        } => {
            let (origin, agent) = origin::authenticated_local_caller(state, headers)?;
            (
                origin,
                OutboundRelayTransport::CloudOutbox {
                    target: CloudOutboxTarget {
                        account_id,
                        source_installation_id,
                        target_installation_id,
                    },
                    origin,
                    agent,
                },
            )
        }
    };
    Ok((request_origin, transport, peer_id, peer_name, peer_version))
}

pub(super) enum ReadyOutboundTransport {
    Http(HttpRelay),
    CloudIngress {
        installation_id: String,
        endpoint: CloudRoute,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
    },
}

impl ReadyOutboundTransport {
    pub(super) fn prepare(
        state: &AppState,
        peer_id: &str,
        peer_name: &str,
        transport: OutboundRelayTransport,
    ) -> Result<Self, (StatusCode, String)> {
        match transport {
            OutboundRelayTransport::Http(client) => Ok(Self::Http(client)),
            OutboundRelayTransport::CloudIngress {
                installation_id,
                endpoint,
                origin,
                agent,
            } => {
                let Some(endpoint) =
                    resolve_cloud_ingress_endpoint(state, peer_id, &installation_id, endpoint)
                else {
                    return Err(peer_unroutable_error(peer_name));
                };
                Ok(Self::CloudIngress {
                    installation_id,
                    endpoint,
                    origin,
                    agent,
                })
            }
            OutboundRelayTransport::CloudOutbox { .. } => Err(peer_unroutable_error(peer_name)),
        }
    }

    pub(super) async fn fetch_identify(&self, state: &AppState) -> Option<serde_json::Value> {
        match self {
            Self::Http(relay) => relay
                .client(state)
                .ok()?
                .auth_get_json("/api/identify")
                .await
                .ok(),
            Self::CloudIngress {
                installation_id,
                endpoint,
                origin,
                agent,
                ..
            } => {
                let token = super::sign_caller(state, *origin, agent.as_ref()).ok()?;
                let command = RelayCommand {
                    method: "GET".into(),
                    path: "/api/identify".into(),
                    body: serde_json::Value::Null,
                    delivery_id: None,
                };
                let resp = send_via_cloud_ingress(
                    state,
                    endpoint.clone(),
                    installation_id,
                    command,
                    token,
                )
                .await
                .ok()?;
                resp.ok.then_some(resp.data).flatten()
            }
        }
    }

    pub(super) async fn transfer_artifacts(
        &self,
        state: &AppState,
        source_tab: Result<Option<String>, String>,
        method: &str,
        path: &str,
        body: &mut serde_json::Value,
    ) -> Result<(), String> {
        match self {
            Self::Http(relay) => {
                codeterm_service_core::service::mesh_artifacts_transfer_via(
                    &state.svc,
                    source_tab,
                    method,
                    path,
                    body,
                    |command| async move {
                        relay
                            .client(state)
                            .map_err(|(_, message)| message)?
                            .send_relay(&command)
                            .await
                            .map_err(|error| error.to_string())
                    },
                )
                .await
            }
            Self::CloudIngress {
                installation_id,
                endpoint,
                origin,
                agent,
                ..
            } => {
                codeterm_service_core::service::mesh_artifacts_transfer_via(
                    &state.svc,
                    source_tab,
                    method,
                    path,
                    body,
                    |command| {
                        let state = state.clone();
                        let installation_id = installation_id.clone();
                        let endpoint = endpoint.clone();
                        let origin = *origin;
                        let agent = agent.clone();
                        async move {
                            let token = super::sign_caller(&state, origin, agent.as_ref())
                                .map_err(|(_, message)| message)?;
                            send_via_cloud_ingress(
                                &state,
                                endpoint,
                                &installation_id,
                                command,
                                token,
                            )
                            .await
                            .map_err(|error| error.to_string())
                        }
                    },
                )
                .await
            }
        }
    }

    pub(super) async fn send_relay(
        self,
        state: &AppState,
        cmd: &RelayCommand,
    ) -> Result<RelayResponse, RelayResponse> {
        match self {
            Self::Http(relay) => {
                let client = relay.client(state).map_err(|(status, message)| {
                    relay_failure(
                        status.as_u16(),
                        codeterm_mesh::contract::delivery::MESH_TRANSPORT_NOTHING_SENT,
                        Some(false),
                        message,
                    )
                })?;
                client.send_relay(cmd).await.map_err(|err| {
                    relay_failure(
                        err.http_status(),
                        err.code(),
                        err.bytes_written(),
                        err.to_string(),
                    )
                })
            }
            Self::CloudIngress {
                installation_id,
                endpoint,
                origin,
                agent,
            } => {
                let sign_state = state.clone();
                let signed_token = tokio::task::spawn_blocking(move || {
                    super::sign_caller(&sign_state, origin, agent.as_ref())
                })
                .await
                .map_err(|error| {
                    relay_failure(
                        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        codeterm_mesh::contract::delivery::MESH_TRANSPORT_NOTHING_SENT,
                        Some(false),
                        error.to_string(),
                    )
                })?
                .map_err(|(status, message)| {
                    relay_failure(
                        status.as_u16(),
                        codeterm_mesh::contract::delivery::MESH_TRANSPORT_NOTHING_SENT,
                        Some(false),
                        message,
                    )
                })?;
                send_via_cloud_ingress(state, endpoint, &installation_id, cmd.clone(), signed_token)
                    .await
                    .map_err(|err| {
                        relay_failure(
                            err.http_status(),
                            err.code(),
                            err.bytes_written(),
                            err.to_string(),
                        )
                    })
            }
        }
    }
}

/// Sends an owner-submitted secure input delivery directly over enrolled Noise
/// (`CloudIngress`) or signed Ed25519 HTTP — never via the cloud outbox.
/// Enforces `(Local, Owner)` authority and zeroizes the payload JSON body immediately after send.
pub(crate) async fn send_owner_secure_delivery(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    target_peer_id: &str,
    request_id: &str,
    body: &mut serde_json::Value,
) -> Result<RelayResponse, RelayResponse> {
    if !matches!(
        crate::auth::authenticated_request(state, headers),
        Ok((
            crate::auth::CallerIdentity::Local,
            AuthenticatedOrigin::Owner
        ))
    ) {
        codeterm_service_core::service::secure_input::zeroize_secure_input_json_value(body);
        return Err(relay_failure(
            StatusCode::FORBIDDEN.as_u16(),
            codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
            Some(false),
            "Only the local human owner may send a secure input delivery".to_owned(),
        ));
    }
    let (request_origin, transport, peer_id, peer_name, _peer_version) =
        match resolve_outbound_transport(
            state,
            headers,
            &(Some(target_peer_id.to_owned()), None),
            false,
        ) {
            Ok(resolved) => resolved,
            Err((status, message)) => {
                codeterm_service_core::service::secure_input::zeroize_secure_input_json_value(body);
                return Err(relay_failure(
                    status.as_u16(),
                    codeterm_mesh::contract::delivery::MESH_TRANSPORT_NOTHING_SENT,
                    Some(false),
                    message,
                ));
            }
        };
    if request_origin != AuthenticatedOrigin::Owner {
        codeterm_service_core::service::secure_input::zeroize_secure_input_json_value(body);
        return Err(relay_failure(
            StatusCode::FORBIDDEN.as_u16(),
            codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
            Some(false),
            "Outbound secure input delivery requires Owner origin".to_owned(),
        ));
    }
    let ready = match ReadyOutboundTransport::prepare(state, &peer_id, &peer_name, transport) {
        Ok(ready) => ready,
        Err((status, message)) => {
            codeterm_service_core::service::secure_input::zeroize_secure_input_json_value(body);
            return Err(relay_failure(
                status.as_u16(),
                codeterm_mesh::contract::delivery::MESH_TRANSPORT_NOTHING_SENT,
                Some(false),
                message,
            ));
        }
    };
    let mut cmd = RelayCommand {
        method: "POST".to_owned(),
        path: "/api/secure-input/deliver".to_owned(),
        body: std::mem::take(body),
        delivery_id: Some(request_id.to_owned()),
    };
    let outcome = ready.send_relay(state, &cmd).await;
    cmd.zeroize_sensitive_body();
    outcome
}

/// Resolves a paired peer for secure input requests using the shared relay/cloud-machine
/// resolution fallback while strictly requiring `AccessLevel::Full` and active traffic.
pub(crate) fn resolve_secure_input_peer(
    state: &AppState,
    selector: &str,
) -> Result<(String, String), (StatusCode, String)> {
    let cloud_machines = state.svc.cloud_machines();
    let mesh = state.mesh.lock();
    let (_, peer_id, peer_name, _) =
        resolve_relay_target_route(&mesh, &cloud_machines, false, selector).map_err(
            |(status, message)| {
                if status == StatusCode::CONFLICT {
                    (status, message)
                } else {
                    (
                StatusCode::FORBIDDEN,
                format!("Target machine '{selector}' is not an authorized online peer: {message}"),
            )
                }
            },
        )?;
    let Some(peer) = mesh.paired_peers.get(&peer_id) else {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Target machine '{selector}' is no longer paired"),
        ));
    };
    if peer.access_level != codeterm_service_core::mesh::types::AccessLevel::Full {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Target machine '{}' does not grant Full access for secure input delivery",
                peer.instance_name
            ),
        ));
    }
    if !mesh.traffic_open(&peer.instance_id) {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Target machine '{}' is currently unavailable or revoked",
                peer.instance_name
            ),
        ));
    }
    Ok((peer_id, peer_name))
}

pub(super) struct CloudOutboxTarget {
    pub account_id: String,
    pub source_installation_id: String,
    pub target_installation_id: String,
}

enum ClassifiedCloudOutbound {
    Operation(OutboundOperation),
    NotificationRefused,
    Unsupported(String),
}

fn classify_cloud_outbound(
    method: &str,
    path: &str,
    body: &serde_json::Value,
) -> ClassifiedCloudOutbound {
    let bare = path.split('?').next().unwrap_or(path);
    if !method.eq_ignore_ascii_case("POST") {
        return ClassifiedCloudOutbound::Unsupported(format!(
            "Cloud peer cannot directly dial {method} {bare}; only outbox-admitted operations are allowed"
        ));
    }
    match bare {
        "/api/notifications" => ClassifiedCloudOutbound::NotificationRefused,
        "/api/mesh/announce" => ClassifiedCloudOutbound::Operation(OutboundOperation::Announce),
        "/api/mesh/rpc" => match body.get("method").and_then(serde_json::Value::as_str) {
            Some(WORKER_ACK_RPC_METHOD) => {
                ClassifiedCloudOutbound::Operation(OutboundOperation::WorkerAck)
            }
            Some(WORKER_REPORT_RPC_METHOD) => {
                ClassifiedCloudOutbound::Operation(OutboundOperation::WorkerReport)
            }
            Some(rpc_method) if is_allowed_plan_rpc_method(rpc_method) => {
                ClassifiedCloudOutbound::Operation(OutboundOperation::PlanRpc)
            }
            Some(rpc_method) => ClassifiedCloudOutbound::Unsupported(format!(
                "Cloud peer cannot queue disallowed RPC method '{rpc_method}'"
            )),
            None => ClassifiedCloudOutbound::Unsupported(
                "Cloud peer RPC request requires a 'method' string".into(),
            ),
        },
        _ => ClassifiedCloudOutbound::Unsupported(format!(
            "Cloud peer cannot directly dial POST {bare}; only outbox-admitted operations are allowed"
        )),
    }
}

fn outbox_error_relay_response(error: OutboxError) -> RelayResponse {
    let status = match &error {
        OutboxError::Full { .. } => StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        OutboxError::PayloadTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
        OutboxError::InvalidRequest { .. } => StatusCode::BAD_REQUEST.as_u16(),
        OutboxError::DuplicateId { .. } => StatusCode::CONFLICT.as_u16(),
        OutboxError::Storage { .. } => StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
    };
    let code = match &error {
        OutboxError::Full { .. } => OUTBOX_FULL,
        OutboxError::PayloadTooLarge { .. } => PAYLOAD_TOO_LARGE,
        other => other.code(),
    };
    relay_failure(status, code, Some(false), error.to_string())
}

fn delivery_unconfirmed_relay_response(
    id: &OutboxId,
    reason: UnconfirmedReason,
    bytes_written: Option<bool>,
    detail: Option<String>,
) -> RelayResponse {
    let normalized_written =
        StreamDeliveryOutcome::normalize_unconfirmed_bytes_written(bytes_written);
    let message = match detail {
        Some(detail) if !detail.trim().is_empty() => format!(
            "cloud outbox record {} delivery unconfirmed ({reason:?}): {detail}",
            id.as_str()
        ),
        _ => format!(
            "cloud outbox record {} delivery unconfirmed ({reason:?})",
            id.as_str()
        ),
    };
    let mut response = relay_failure(
        StatusCode::GATEWAY_TIMEOUT.as_u16(),
        DELIVERY_UNCONFIRMED,
        normalized_written,
        message,
    );
    if let Some(obj) = response
        .data
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    {
        obj.insert("outbox_id".into(), serde_json::json!(id.as_str()));
        obj.insert("reason".into(), serde_json::json!(reason));
    }
    response
}

fn decode_delivered_reply(
    operation: OutboundOperation,
    id: &OutboxId,
    reply_body: Option<Vec<u8>>,
) -> RelayResponse {
    match operation {
        OutboundOperation::Announce => {
            let data = reply_body
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
                .unwrap_or_else(|| serde_json::json!({ "ok": true }));
            RelayResponse {
                ok: true,
                status: StatusCode::OK.as_u16(),
                data: Some(data),
                error: None,
            }
        }
        OutboundOperation::WorkerAck
        | OutboundOperation::WorkerReport
        | OutboundOperation::PlanRpc
        | OutboundOperation::Notification => {
            let Some(bytes) = reply_body else {
                return delivery_unconfirmed_relay_response(
                    id,
                    UnconfirmedReason::ReceiverStateUncertain,
                    Some(true),
                    Some("missing application reply body from home peer".into()),
                );
            };
            match serde_json::from_slice::<RelayResponse>(&bytes) {
                Ok(reply) => reply,
                Err(error) => delivery_unconfirmed_relay_response(
                    id,
                    UnconfirmedReason::ReceiverStateUncertain,
                    Some(true),
                    Some(format!("invalid application reply from home peer: {error}")),
                ),
            }
        }
    }
}

pub(super) async fn relay_via_cloud_outbox(
    state: &AppState,
    target: CloudOutboxTarget,
    origin: AuthenticatedOrigin,
    agent: Option<AuthenticatedAgent>,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<RelayResponse, (StatusCode, String)> {
    let operation = match classify_cloud_outbound(method, path, &body) {
        ClassifiedCloudOutbound::Operation(op) => op,
        ClassifiedCloudOutbound::NotificationRefused => {
            return Ok(relay_failure(
                StatusCode::FORBIDDEN.as_u16(),
                CLOUD_NOTIFICATION_RELAY_REFUSED,
                Some(false),
                "Relayed notifications from cloud peers are refused pending security review".into(),
            ));
        }
        ClassifiedCloudOutbound::Unsupported(message) => {
            return Ok(relay_failure(
                StatusCode::FORBIDDEN.as_u16(),
                CLOUD_OUTBOUND_OPERATION_UNSUPPORTED,
                Some(false),
                message,
            ));
        }
    };

    if matches!(
        operation,
        OutboundOperation::PlanRpc | OutboundOperation::Announce
    ) && !origin.owner_trusted()
    {
        return Ok(relay_failure(
            StatusCode::FORBIDDEN.as_u16(),
            codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
            Some(false),
            format!("{} requires an owner-trusted origin", operation.as_str()),
        ));
    }

    let body_bytes = serde_json::to_vec(&body).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to serialize cloud outbound body: {error}"),
        )
    })?;
    let pair = InstallationPair::new(
        target.source_installation_id.clone(),
        target.target_installation_id.clone(),
    );
    let outbound = OutboundPeerRequest {
        account_id: target.account_id,
        source_installation_id: target.source_installation_id,
        target_installation_id: target.target_installation_id,
        operation,
        caller_context: CallerContext {
            origin,
            agent,
            expires_at_ms: None,
        },
        body: body_bytes,
    };

    let outbox = resolve_cloud_outbox(state).await?;
    let receipt = match outbox.enqueue(outbound).await {
        Ok(receipt) => receipt,
        Err(error) => return Ok(outbox_error_relay_response(error)),
    };

    let settled = match tokio::time::timeout(
        CLOUD_OUTBOX_WAIT_DEADLINE,
        outbox.wait_for_settlement(&receipt.id, &pair),
    )
    .await
    {
        Ok(status) => status,
        Err(_) => outbox.record_status(&receipt.id),
    };

    match settled {
        Some(OutboxRecordStatus::Delivered { .. }) => {
            let reply = outbox
                .application_reply(&receipt.id)
                .and_then(|completed| completed.body);
            Ok(decode_delivered_reply(operation, &receipt.id, reply))
        }
        Some(OutboxRecordStatus::DeliveryUnconfirmed {
            reason,
            bytes_written,
            detail,
            ..
        }) => Ok(delivery_unconfirmed_relay_response(
            &receipt.id,
            reason,
            bytes_written,
            detail,
        )),
        Some(OutboxRecordStatus::RefusedBeforeWrite { reason, .. }) => {
            let code = if reason == CLOUD_NOTIFICATION_RELAY_REFUSED {
                CLOUD_NOTIFICATION_RELAY_REFUSED
            } else {
                REFUSED_BEFORE_WRITE
            };
            Ok(relay_failure(
                StatusCode::FORBIDDEN.as_u16(),
                code,
                Some(false),
                format!("cloud outbox refused before write: {reason}"),
            ))
        }
        Some(OutboxRecordStatus::Queued { .. }) => Ok(RelayResponse {
            ok: true,
            status: StatusCode::ACCEPTED.as_u16(),
            data: Some(serde_json::json!({
                "ok": true,
                "queued": true,
                "bytes_written": false,
                "outbox_id": receipt.id.as_str(),
                "sequence": receipt.sequence,
                "status": "queued",
            })),
            error: None,
        }),
        Some(OutboxRecordStatus::InFlight { .. }) | None => {
            Ok(delivery_unconfirmed_relay_response(
                &receipt.id,
                UnconfirmedReason::AckTimeout,
                None,
                Some("cloud outbox delivery did not settle before deadline".into()),
            ))
        }
    }
}

/// Routes `mesh_announce_all` through `CloudOutboxPort::enqueue_broadcast` when
/// running on an inbound-only cloud peer, so it never dials out directly.
pub(crate) async fn cloud_outbox_announce(
    state: &AppState,
    target: Option<&str>,
) -> Option<AnnounceReport> {
    if !has_cloud_outbox(state) {
        return None;
    }
    let (base, target_installations) = {
        let mesh = state.mesh.lock();
        let Some(account_id) = mesh.account_state().signed_in_account().map(str::to_owned) else {
            return Some(AnnounceReport {
                errors: vec!["cloud outbox announce: account not signed in".into()],
                ..Default::default()
            });
        };
        let Some(source_installation_id) = mesh
            .account_state()
            .current_membership(mesh.now())
            .map(|m| m.verified.installation_id.clone())
        else {
            return Some(AnnounceReport {
                errors: vec!["cloud outbox announce: local membership unavailable".into()],
                ..Default::default()
            });
        };
        let announce = mesh.build_announce();
        let Ok(body) = serde_json::to_vec(&announce) else {
            return Some(AnnounceReport {
                errors: vec!["cloud outbox announce: failed to serialize announce".into()],
                ..Default::default()
            });
        };
        let resolved_target_id = target.and_then(|target| {
            mesh.resolve_peer_including_offline(target)
                .ok()
                .map(|peer| peer.instance_id.as_str())
        });
        let mut targets: Vec<String> = mesh
            .paired_peers
            .values()
            .filter(|peer| match target {
                None => true,
                Some(target) => {
                    resolved_target_id == Some(peer.instance_id.as_str())
                        || target == peer.instance_id
                        || target == peer.instance_name
                }
            })
            .filter(|peer| mesh.announce_allowed(&peer.instance_id))
            .filter_map(|peer| {
                mesh.link_record(&peer.instance_id)
                    .and_then(|link| link.installation_id())
                    .map(str::to_owned)
            })
            .collect();
        targets.sort();
        targets.dedup();
        let placeholder_target = targets
            .first()
            .cloned()
            .unwrap_or_else(|| source_installation_id.clone());
        let base = OutboundPeerRequest {
            account_id,
            source_installation_id,
            target_installation_id: placeholder_target,
            operation: OutboundOperation::Announce,
            caller_context: CallerContext {
                origin: AuthenticatedOrigin::Mesh,
                agent: None,
                expires_at_ms: None,
            },
            body,
        };
        (base, targets)
    };
    if target_installations.is_empty() {
        return Some(AnnounceReport::default());
    }
    let outbox = match resolve_cloud_outbox(state).await {
        Ok(outbox) => outbox,
        Err((_, error)) => {
            return Some(AnnounceReport {
                errors: vec![error],
                ..Default::default()
            })
        }
    };
    let count = target_installations.len();
    match outbox.enqueue_broadcast(base, target_installations).await {
        Ok(receipts) => Some(AnnounceReport {
            sent: count,
            ok: receipts.len(),
            stale_peers: Vec::new(),
            errors: Vec::new(),
        }),
        Err(error) => Some(AnnounceReport {
            sent: count,
            ok: 0,
            stale_peers: Vec::new(),
            errors: vec![error.to_string()],
        }),
    }
}
