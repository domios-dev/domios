use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_mesh::cloud_ingress::CloudRoute;
use codeterm_neighbors_service::service::cloud_installation::controller::{
    enroll as baker_enroll, ControllerError, CreationBinding,
};
use codeterm_service_core::service::cloud_machines::{
    CloudBillingMode, CloudError, CloudLimitPolicy, CloudMachineRecord, CloudPeerControlPort,
    CloudRouteSpec, CloudTarget, CreateCloudMachineParams, DrainToken, EnrollmentBinding,
    EnrollmentOutcome, FlushWriteOutcome, HeldCloudSend, PeerBusySnapshot, PluginCloudBackend,
    DEFAULT_WAKE_HOLD_DEADLINE_MS,
};
use serde::Deserialize;

use crate::routes::AppState;

struct BakerEnrollmentAdapter {
    svc: codeterm_service_core::service::ServiceState,
}

#[async_trait]
impl codeterm_service_core::service::cloud_machines::CloudEnrollmentPort
    for BakerEnrollmentAdapter
{
    async fn prepare_binding(&self) -> Result<EnrollmentBinding, CloudError> {
        let binding = CreationBinding::new(&self.svc)
            .await
            .map_err(|error| CloudError::EnrollmentFailed(format!("{error:?}")))?;
        let controller_key = binding.controller_key();
        let provider_creation_nonce = binding.provider_creation_nonce();
        Ok(EnrollmentBinding {
            controller_key,
            provider_creation_nonce,
            opaque: Arc::new(binding),
        })
    }

    async fn enroll(
        &self,
        route: &CloudRouteSpec,
        binding: &EnrollmentBinding,
        provider: &str,
        size: &str,
    ) -> Result<EnrollmentOutcome, CloudError> {
        let creation = binding
            .opaque
            .downcast_ref::<CreationBinding>()
            .ok_or_else(|| CloudError::EnrollmentFailed("invalid creation binding".into()))?;
        let cloud_route = match route {
            CloudRouteSpec::Direct { address } => CloudRoute::Direct(*address),
            CloudRouteSpec::Preview { .. } => {
                return Err(CloudError::EnrollmentFailed(
                    "preview cloud route is unsupported; direct ingress required".into(),
                ));
            }
        };
        match baker_enroll(&self.svc, cloud_route, creation, provider, size).await {
            Ok(installation_id) => Ok(EnrollmentOutcome::Enrolled {
                installation_id,
                peer_instance_id: None,
            }),
            Err(ControllerError::ApplyUnconfirmed { installation_id }) => {
                Ok(EnrollmentOutcome::ApplyUnconfirmed { installation_id })
            }
            Err(error) => Err(CloudError::EnrollmentFailed(format!("{error:?}"))),
        }
    }
}

pub struct MeshPeerControlAdapter {
    state: AppState,
}

impl MeshPeerControlAdapter {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn peer_client(
        &self,
        record: &CloudMachineRecord,
    ) -> Result<codeterm_service_core::mesh::transport::PeerClient, CloudError> {
        let mesh = self.state.mesh.lock();
        let target = record
            .peer_instance_id
            .as_deref()
            .or(record.installation_id.as_deref())
            .unwrap_or(&record.machine_id);
        let peer_id = mesh
            .resolve_peer(target)
            .map(|peer| peer.instance_id.clone())
            .map_err(|_| CloudError::ProviderUnavailable(format!("peer {target} is not online")))?;
        mesh.client_for(&peer_id)
            .ok_or_else(|| CloudError::ProviderUnavailable(format!("peer {target} has no route")))
    }

    fn cloud_ingress_target(
        &self,
        record: &CloudMachineRecord,
    ) -> Option<(String, String, Option<std::net::SocketAddrV4>, String)> {
        let mesh = self.state.mesh.lock();
        let mut resolved_peer_id = None;
        for selector in [
            record.peer_instance_id.as_deref(),
            record.installation_id.as_deref(),
            Some(record.machine_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(peer) = mesh.resolve_peer_including_offline(selector) {
                resolved_peer_id = Some(peer.instance_id.clone());
                break;
            }
        }
        let peer_id = resolved_peer_id?;
        if !mesh.traffic_open(&peer_id) {
            return None;
        }
        let installation_id = mesh
            .link_record(&peer_id)
            .and_then(|link| link.installation_id())
            .map(str::to_owned)
            .or_else(|| record.installation_id.clone())?;
        let known_address = mesh.direct_ingress_address(&installation_id);
        if known_address.is_none() && mesh.peer_route_endpoints(&peer_id).is_some() {
            return None;
        }
        let token = mesh.sign_request().ok()?;
        Some((peer_id, installation_id, known_address, token))
    }

    async fn send_cloud_relay(
        &self,
        record: &CloudMachineRecord,
        cmd: codeterm_service_core::mesh::types::RelayCommand,
    ) -> Option<
        Result<
            codeterm_service_core::mesh::types::RelayResponse,
            crate::mesh_routes::relay::CloudIngressSendError,
        >,
    > {
        let (peer_id, installation_id, known, token) = self.cloud_ingress_target(record)?;
        let address = crate::mesh_routes::relay::resolve_cloud_ingress_address(
            &self.state,
            &peer_id,
            &installation_id,
            known,
        )
        .await?;
        Some(
            crate::mesh_routes::relay::send_via_cloud_ingress(
                &self.state,
                address,
                &installation_id,
                cmd,
                token,
            )
            .await,
        )
    }

    fn peer_is_online(&self, record: &CloudMachineRecord) -> bool {
        let mesh = self.state.mesh.lock();
        let target = record
            .peer_instance_id
            .as_deref()
            .or(record.installation_id.as_deref())
            .unwrap_or(&record.machine_id);
        if mesh.resolve_peer(target).is_ok_and(|peer| peer.online) {
            return true;
        }
        let Ok(peer) = mesh.resolve_peer_including_offline(target) else {
            return false;
        };
        if !mesh.traffic_open(&peer.instance_id) {
            return false;
        }
        let installation_id = mesh
            .link_record(&peer.instance_id)
            .and_then(|link| link.installation_id())
            .or(record.installation_id.as_deref());
        installation_id.is_some_and(|id| mesh.direct_ingress_address(id).is_some())
    }
}

#[async_trait]
impl CloudPeerControlPort for MeshPeerControlAdapter {
    async fn read_idle(&self, record: &CloudMachineRecord) -> Result<PeerBusySnapshot, CloudError> {
        let value = if let Some(cloud_result) = self
            .send_cloud_relay(
                record,
                codeterm_service_core::mesh::types::RelayCommand {
                    method: "GET".into(),
                    path: "/api/mesh/cloud/peer/idle".into(),
                    body: serde_json::Value::Null,
                    delivery_id: None,
                },
            )
            .await
        {
            let resp =
                cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
            if !resp.ok {
                return Err(CloudError::ProviderUnavailable(resp.error.unwrap_or_else(
                    || format!("cloud peer idle returned status {}", resp.status),
                )));
            }
            resp.data.unwrap_or(serde_json::Value::Null)
        } else {
            let client = self.peer_client(record)?;
            client
                .auth_get_json("/api/mesh/cloud/peer/idle")
                .await
                .map_err(CloudError::ProviderUnavailable)?
        };
        serde_json::from_value(value).map_err(|error| CloudError::Internal(error.to_string()))
    }

    async fn enter_drain(&self, record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        let value = if let Some(cloud_result) = self
            .send_cloud_relay(
                record,
                codeterm_service_core::mesh::types::RelayCommand {
                    method: "POST".into(),
                    path: "/api/mesh/cloud/peer/drain".into(),
                    body: serde_json::json!({}),
                    delivery_id: None,
                },
            )
            .await
        {
            let resp =
                cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
            if !resp.ok {
                let detail = resp
                    .error
                    .unwrap_or_else(|| format!("status {}", resp.status));
                return Err(if resp.status == 409 || detail.contains("machine_busy") {
                    CloudError::MachineBusy {
                        machine_id: record.machine_id.clone(),
                        reason: detail,
                    }
                } else {
                    CloudError::ProviderUnavailable(detail)
                });
            }
            resp.data.unwrap_or(serde_json::Value::Null)
        } else {
            let client = self.peer_client(record)?;
            client
                .auth_post_json("/api/mesh/cloud/peer/drain", &serde_json::json!({}))
                .await
                .map_err(|error| {
                    if error.contains("machine_busy") || error.contains("409") {
                        CloudError::MachineBusy {
                            machine_id: record.machine_id.clone(),
                            reason: error,
                        }
                    } else {
                        CloudError::ProviderUnavailable(error)
                    }
                })?
        };
        serde_json::from_value(value).map_err(|error| CloudError::Internal(error.to_string()))
    }

    async fn reopen_drain(&self, record: &CloudMachineRecord) -> Result<(), CloudError> {
        if let Some(cloud_result) = self
            .send_cloud_relay(
                record,
                codeterm_service_core::mesh::types::RelayCommand {
                    method: "POST".into(),
                    path: "/api/mesh/cloud/peer/reopen".into(),
                    body: serde_json::json!({}),
                    delivery_id: None,
                },
            )
            .await
        {
            let resp =
                cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
            return if resp.ok {
                Ok(())
            } else {
                Err(CloudError::ProviderUnavailable(resp.error.unwrap_or_else(
                    || format!("cloud peer reopen returned status {}", resp.status),
                )))
            };
        }
        let client = self.peer_client(record)?;
        client
            .auth_post_json("/api/mesh/cloud/peer/reopen", &serde_json::json!({}))
            .await
            .map(|_| ())
            .map_err(CloudError::ProviderUnavailable)
    }

    async fn await_peer_ready(
        &self,
        record: &CloudMachineRecord,
        deadline_ms: u64,
    ) -> Result<(), CloudError> {
        let mut rx = self.state.events.subscribe();
        let mut gate = self.state.mesh.lock().gate_watch();
        loop {
            if self.peer_is_online(record) {
                return Ok(());
            }
            let now_ms = self.state.svc.cloud_machines().now_ms();
            if now_ms >= deadline_ms {
                return Err(CloudError::WakeDeadlineExceeded {
                    machine_id: record.machine_id.clone(),
                });
            }
            let remaining = Duration::from_millis(deadline_ms.saturating_sub(now_ms));
            let wait = async {
                tokio::select! {
                    res = rx.recv() => match res {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => false,
                    },
                    res = gate.changed() => res.is_ok(),
                }
            };
            match tokio::time::timeout(remaining, wait).await {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    return Err(CloudError::WakeDeadlineExceeded {
                        machine_id: record.machine_id.clone(),
                    });
                }
            }
        }
    }

    async fn flush_send(
        &self,
        record: &CloudMachineRecord,
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        let cmd = codeterm_service_core::mesh::types::RelayCommand {
            method: send.method.clone(),
            path: send.path.clone(),
            body: send.body.clone(),
            delivery_id: Some(send.delivery_id.clone()),
        };
        let result = if let Some(cloud_result) = self.send_cloud_relay(record, cmd.clone()).await {
            cloud_result.map_err(|error| (error.bytes_written(), error.to_string()))
        } else {
            let Ok(client) = self.peer_client(record) else {
                return FlushWriteOutcome::RefusedMachineSleeping;
            };
            client
                .send_relay(&cmd)
                .await
                .map_err(|error| (error.bytes_written(), error.to_string()))
        };
        match result {
            Ok(resp) if resp.ok => {
                let delivered = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("delivery"))
                    .and_then(serde_json::Value::as_str)
                    == Some("sent");
                if delivered {
                    FlushWriteOutcome::Delivered
                } else {
                    FlushWriteOutcome::Written
                }
            }
            Ok(resp) => {
                let err_code = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.pointer("/error/code"))
                    .and_then(serde_json::Value::as_str);
                let bytes_written = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.pointer("/error/bytes_written"))
                    .and_then(serde_json::Value::as_bool);
                if err_code == Some("machine_sleeping") && bytes_written == Some(false) {
                    return FlushWriteOutcome::RefusedMachineSleeping;
                }
                if bytes_written == Some(false) {
                    return FlushWriteOutcome::RefusedPreWrite(
                        resp.error
                            .or_else(|| err_code.map(str::to_owned))
                            .unwrap_or_else(|| "refused_before_write".into()),
                    );
                }
                FlushWriteOutcome::Uncertain(
                    resp.error
                        .unwrap_or_else(|| format!("status {}", resp.status)),
                )
            }
            Err((bytes_written, detail)) => {
                if bytes_written == Some(false) {
                    FlushWriteOutcome::RefusedPreWrite(detail)
                } else {
                    FlushWriteOutcome::Uncertain(detail)
                }
            }
        }
    }
}

fn limit_policy(state: &AppState) -> CloudLimitPolicy {
    let limit = state.config.lock().mesh.cloud_machine_limit;
    CloudLimitPolicy {
        max_machines: limit,
        mode: CloudBillingMode::Byo,
        allow_managed: false,
    }
}

fn error_response(error: CloudError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match &error {
        CloudError::LimitReached { .. }
        | CloudError::FreePlanManagedRefused
        | CloudError::ImplicitCreateRefused => StatusCode::FORBIDDEN,
        CloudError::MachineNotFound(_) | CloudError::ProviderNotFound(_) => StatusCode::NOT_FOUND,
        CloudError::MachineAmbiguous(_)
        | CloudError::MachineBusy { .. }
        | CloudError::InvalidTransition { .. }
        | CloudError::DeleteConfirmationRequired { .. }
        | CloudError::SendAlreadyClaimed(_) => StatusCode::CONFLICT,
        CloudError::MachineOutOfCredits { .. } => StatusCode::PAYMENT_REQUIRED,
        CloudError::MachineSleeping { .. }
        | CloudError::ProviderUnavailable(_)
        | CloudError::WakeDeadlineExceeded { .. }
        | CloudError::ApplyUnconfirmed { .. } => StatusCode::SERVICE_UNAVAILABLE,
        CloudError::EnrollmentFailed(_) | CloudError::Internal(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    let mut body = serde_json::json!({
        "ok": false,
        "error": {
            "code": error.code(),
            "message": error.to_string(),
        }
    });
    if let Some(bytes_written) = error.bytes_written() {
        body["error"]["bytes_written"] = serde_json::json!(bytes_written);
    }
    (status, Json(body))
}

fn require_local_caller(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if crate::auth::is_local_caller(state, headers) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "local_only",
                    "message": "Cloud controller mutations require a local caller",
                }
            })),
        ))
    }
}

pub(crate) async fn wake_machine_for_use(
    state: &AppState,
    machine_id: &str,
) -> Result<codeterm_service_core::service::cloud_machines::CloudReceipt, CloudError> {
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(machine_id)
        .ok_or_else(|| CloudError::MachineNotFound(machine_id.to_owned()))?;
    let (_, backend) =
        PluginCloudBackend::resolve_from_service(&state.svc, Some(&record.provider))?;
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    ctrl.wake(
        &record.machine_id,
        backend.as_ref(),
        &peer_ctrl,
        DEFAULT_WAKE_HOLD_DEADLINE_MS,
    )
    .await
}

pub(crate) fn spawn_wake_and_flush(state: AppState, machine_id: String) {
    tokio::spawn(async move {
        if let Err(error) = wake_machine_for_use(&state, &machine_id).await {
            log::warn!("[cloud] wake-on-use failed for {machine_id}: {error}");
        }
    });
}

#[derive(Deserialize, Default)]
pub struct CreateCloudBody {
    pub provider: Option<String>,
    pub size: Option<String>,
    pub name: Option<String>,
    pub auto_stop_minutes: Option<u32>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

async fn create_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateCloudBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let (provider_id, backend) =
        PluginCloudBackend::resolve_from_service(&state.svc, body.provider.as_deref())
            .map_err(error_response)?;
    let enrollment = BakerEnrollmentAdapter {
        svc: state.svc.clone(),
    };
    let auto_stop_minutes = body
        .auto_stop_minutes
        .unwrap_or_else(|| state.config.lock().mesh.cloud_auto_stop_minutes);
    let receipt = state
        .svc
        .cloud_machines()
        .create(
            CreateCloudMachineParams {
                provider: provider_id,
                size: body.size.unwrap_or_else(|| "small".into()),
                name: body.name,
                auto_stop_minutes,
                extra_labels: body.labels,
            },
            backend.as_ref(),
            &enrollment,
            limit_policy(&state),
        )
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

async fn list_machines(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let machines = state.svc.cloud_machines().list();
    let limit = state.config.lock().mesh.cloud_machine_limit;
    Ok(Json(serde_json::json!({
        "machines": machines,
        "limit": limit,
    })))
}

async fn sleep_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let (_, backend) = PluginCloudBackend::resolve_from_service(&state.svc, Some(&record.provider))
        .map_err(error_response)?;
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let receipt = ctrl
        .sleep(&record.machine_id, backend.as_ref(), &peer_ctrl, false)
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

async fn wake_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let receipt = wake_machine_for_use(&state, &id)
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub struct DeleteCloudBody {
    #[serde(default)]
    pub confirm: bool,
}

async fn delete_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<DeleteCloudBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let (_, backend) = PluginCloudBackend::resolve_from_service(&state.svc, Some(&record.provider))
        .map_err(error_response)?;
    let receipt = ctrl
        .delete(&record.machine_id, body.confirm, backend.as_ref())
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub struct ResolveCloudBody {
    #[serde(default)]
    pub target: String,
    pub provider: Option<String>,
    pub size: Option<String>,
    pub name: Option<String>,
}

async fn resolve_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResolveCloudBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let target = CloudTarget::parse(&body.target);
    let ctrl = state.svc.cloud_machines();
    let policy = limit_policy(&state);
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let enrollment = BakerEnrollmentAdapter {
        svc: state.svc.clone(),
    };
    let (provider_id, backend) = match &target {
        CloudTarget::New => {
            PluginCloudBackend::resolve_from_service(&state.svc, body.provider.as_deref())
                .map_err(error_response)?
        }
        _ => {
            let existing = ctrl.list_records();
            let chosen_provider = body
                .provider
                .clone()
                .or_else(|| existing.first().map(|m| m.provider.clone()));
            PluginCloudBackend::resolve_from_service(&state.svc, chosen_provider.as_deref())
                .map_err(error_response)?
        }
    };
    let auto_stop_minutes = state.config.lock().mesh.cloud_auto_stop_minutes;
    let create_params = matches!(target, CloudTarget::New).then(|| CreateCloudMachineParams {
        provider: provider_id,
        size: body.size.unwrap_or_else(|| "small".into()),
        name: body.name,
        auto_stop_minutes,
        extra_labels: BTreeMap::new(),
    });
    let receipt = ctrl
        .resolve_placement(
            &target,
            create_params,
            backend.as_ref(),
            &enrollment,
            &peer_ctrl,
            policy,
        )
        .await
        .map_err(error_response)?;
    let peer_target = receipt
        .peer_instance_id
        .clone()
        .or_else(|| receipt.installation_id.clone())
        .unwrap_or_else(|| receipt.machine_id.clone());
    Ok(Json(serde_json::json!({
        "peer": peer_target,
        "machine": receipt,
    })))
}

async fn usage_summary(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let machines = state.svc.cloud_machines().list();
    let total_uptime_secs: u64 = machines.iter().map(|m| m.uptime_secs).sum();
    Ok(Json(serde_json::json!({
        "mode": "byo_estimate",
        "managed_credits": "reserved_for_tally",
        "total_uptime_secs": total_uptime_secs,
        "machines": machines,
    })))
}

async fn peer_idle(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    let viewers = crate::stream_census::STREAMS.live_count();
    let snapshot = state.svc.cloud_peer_busy_snapshot(viewers);
    let draining = state.svc.cloud_machines().local_drain_gate().is_draining();
    Json(serde_json::json!({
        "active_agent_turns": snapshot.active_agent_turns,
        "running_background_jobs": snapshot.running_background_jobs,
        "held_sends": snapshot.held_sends,
        "attached_viewers": snapshot.attached_viewers,
        "idle": snapshot.is_idle(),
        "draining": draining,
    }))
}

async fn peer_drain(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let viewers = crate::stream_census::STREAMS.live_count();
    let snapshot = state.svc.cloud_peer_busy_snapshot(viewers);
    let now_ms = state.svc.cloud_machines().now_ms();
    let machine_id = state.mesh.lock().status().instance_id;
    let token = state
        .svc
        .cloud_machines()
        .local_drain_gate()
        .try_enter_drain(&machine_id, snapshot, now_ms)
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(token).unwrap_or_default()))
}

async fn peer_reopen(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    state.svc.cloud_machines().local_drain_gate().reopen();
    Json(serde_json::json!({ "ok": true, "draining": false }))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/mesh/cloud/create", post(create_machine))
        .route("/api/mesh/cloud/list", get(list_machines))
        .route("/api/mesh/cloud/{id}/sleep", post(sleep_machine))
        .route("/api/mesh/cloud/{id}/wake", post(wake_machine))
        .route("/api/mesh/cloud/{id}/delete", post(delete_machine))
        .route("/api/mesh/cloud/resolve", post(resolve_machine))
        .route("/api/mesh/cloud/usage", get(usage_summary))
        .route("/api/mesh/cloud/peer/idle", get(peer_idle))
        .route("/api/mesh/cloud/peer/drain", post(peer_drain))
        .route("/api/mesh/cloud/peer/reopen", post(peer_reopen))
}
