use std::net::SocketAddrV4;
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
use codeterm_service_core::{
    mesh::{
        crown::{AuthenticatedAgent, AuthenticatedOrigin},
        types::{RelayCommand, RelayResponse},
        AnnounceReport,
    },
    service::cloud_machines::{
        CloudBackendPort, CloudRouteSpec, PluginCloudBackend, CLOUD_INGRESS_PORT,
    },
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
#[derive(Clone)]
pub struct CloudIngressConnectorOverride(pub Arc<dyn CloudIngressConnector>);

/// Optional host extension allowing registered-router tests to bind a test
/// `CloudOutboxPort` on `ServiceContext` without opening a live ingress socket.
#[derive(Clone)]
pub struct CloudOutboxOverride(pub Arc<dyn CloudOutboxPort>);

pub(crate) async fn send_via_cloud_ingress(
    state: &AppState,
    address: SocketAddrV4,
    installation_id: &str,
    command: RelayCommand,
    signed_token: String,
) -> Result<RelayResponse, CloudIngressSendError> {
    let route = CloudRoute::Direct(address);
    let override_connector = state
        .svc
        .try_ext::<CloudIngressConnectorOverride>()
        .map(|ext| Arc::clone(&ext.0));
    if let Some(connector) = override_connector {
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
}

/// Resolves an enrolled cloud peer's direct ingress address from gossiped
/// endpoint records first, falling back to Warden's managed backend route.
pub(crate) async fn resolve_cloud_ingress_address(
    state: &AppState,
    peer_id: &str,
    installation_id: &str,
    known: Option<SocketAddrV4>,
) -> Option<SocketAddrV4> {
    if let Some(address) = known {
        return Some(address);
    }
    let machines = state.svc.cloud_machines();
    let record = machines
        .find_by_peer(peer_id)
        .or_else(|| machines.find_by_peer(installation_id))?;
    if record.ownership.provider_machine_id.trim().is_empty() {
        return None;
    }
    let (_, backend) =
        PluginCloudBackend::resolve_from_service(&state.svc, Some(&record.provider)).ok()?;
    match backend
        .access(&record.ownership.provider_machine_id, CLOUD_INGRESS_PORT)
        .await
    {
        Ok(CloudRouteSpec::Direct { address }) => Some(address),
        _ => None,
    }
}

pub(crate) fn has_cloud_outbox(state: &AppState) -> bool {
    state.svc.try_ext::<CloudOutboxOverride>().is_some()
        || state.svc.try_ext::<CloudIngressHandle>().is_some()
}

async fn resolve_cloud_outbox(
    state: &AppState,
) -> Result<Arc<dyn CloudOutboxPort>, (StatusCode, String)> {
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
    Http(codeterm_service_core::mesh::transport::PeerClient),
    CloudIngress {
        installation_id: String,
        address: Option<SocketAddrV4>,
        signed_token: String,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
    },
    CloudOutbox {
        target: CloudOutboxTarget,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
    },
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
            let (origin, client) =
                origin::signed_peer_client(state, headers, primary, alternatives)?;
            (origin, OutboundRelayTransport::Http(client))
        }
        RelayTargetRoute::CloudIngress {
            installation_id,
            address,
        } => {
            let (origin, agent, signed_token) = origin::signed_caller_token(state, headers)?;
            (
                origin,
                OutboundRelayTransport::CloudIngress {
                    installation_id,
                    address,
                    signed_token,
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
    Http(codeterm_service_core::mesh::transport::PeerClient),
    CloudIngress {
        installation_id: String,
        address: SocketAddrV4,
        signed_token: String,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
    },
}

impl ReadyOutboundTransport {
    pub(super) async fn prepare(
        state: &AppState,
        peer_id: &str,
        peer_name: &str,
        transport: OutboundRelayTransport,
    ) -> Result<Self, (StatusCode, String)> {
        match transport {
            OutboundRelayTransport::Http(client) => Ok(Self::Http(client)),
            OutboundRelayTransport::CloudIngress {
                installation_id,
                address,
                signed_token,
                origin,
                agent,
            } => {
                let Some(address) =
                    resolve_cloud_ingress_address(state, peer_id, &installation_id, address).await
                else {
                    return Err(peer_unroutable_error(peer_name));
                };
                Ok(Self::CloudIngress {
                    installation_id,
                    address,
                    signed_token,
                    origin,
                    agent,
                })
            }
            OutboundRelayTransport::CloudOutbox { .. } => Err(peer_unroutable_error(peer_name)),
        }
    }

    pub(super) async fn fetch_identify(&self, state: &AppState) -> Option<serde_json::Value> {
        match self {
            Self::Http(client) => client.auth_get_json("/api/identify").await.ok(),
            Self::CloudIngress {
                installation_id,
                address,
                origin,
                agent,
                ..
            } => {
                let token = origin::sign(state, *origin, agent.as_ref()).ok()?;
                let command = RelayCommand {
                    method: "GET".into(),
                    path: "/api/identify".into(),
                    body: serde_json::Value::Null,
                    delivery_id: None,
                };
                let resp = send_via_cloud_ingress(state, *address, installation_id, command, token)
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
            Self::Http(client) => {
                codeterm_service_core::service::mesh_artifacts_transfer(
                    &state.svc, source_tab, client, method, path, body,
                )
                .await
            }
            Self::CloudIngress {
                installation_id,
                address,
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
                        let address = *address;
                        let origin = *origin;
                        let agent = agent.clone();
                        async move {
                            let token = origin::sign(&state, origin, agent.as_ref())
                                .map_err(|(_, message)| message)?;
                            send_via_cloud_ingress(
                                &state,
                                address,
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
            Self::Http(client) => client.send_relay(cmd).await.map_err(|err| {
                relay_failure(
                    err.http_status(),
                    err.code(),
                    err.bytes_written(),
                    err.to_string(),
                )
            }),
            Self::CloudIngress {
                installation_id,
                address,
                signed_token,
                ..
            } => {
                send_via_cloud_ingress(state, address, &installation_id, cmd.clone(), signed_token)
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
