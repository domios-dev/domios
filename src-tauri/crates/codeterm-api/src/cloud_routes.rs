mod adapters;
mod provisioning_adapter;
mod warm_bootstrap;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_service_core::service::cloud_machines::{
    spawn_cloud_idle_driver, CloudDeleteMode, CloudError, CloudIdleDriverConfig,
    CloudIdleDriverHandle, CloudProvisioningPort, CloudTarget, CreateCloudMachineParams,
    TokioIdleTickWaiter, AUX_KEY_STORAGE_TIER, DEFAULT_IDLE_SLEEP_MINUTES,
    DEFAULT_WAKE_HOLD_DEADLINE_MS,
};
use serde::Deserialize;

use crate::routes::AppState;

use adapters::BakerEnrollmentAdapter;
pub use adapters::MeshPeerControlAdapter;
#[cfg(any(test, feature = "test-support"))]
pub use adapters::{CloudEndpointPublisherOverride, CloudProvisioningOverride};
use provisioning_adapter::AccountCloudProvisioningAdapter;

pub(crate) fn register_extensions(
    #[cfg_attr(not(any(test, feature = "test-support")), allow(unused_variables))]
    builder: &mut codeterm_service_core::service::ExtensionsBuilder,
) {
    #[cfg(any(test, feature = "test-support"))]
    {
        builder.insert_late::<CloudProvisioningOverride>();
        builder.insert_late::<CloudEndpointPublisherOverride>();
    }
}

fn resolve_provisioning(state: &AppState) -> Arc<dyn CloudProvisioningPort> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(ext) = state.svc.try_ext::<CloudProvisioningOverride>() {
        return Arc::clone(&ext.0);
    }
    Arc::new(AccountCloudProvisioningAdapter {
        svc: state.svc.clone(),
    })
}

/// Start the home-side per-machine driver for every awake cloud machine owned by this controller.
/// Rebuilds on home Domios restart from `cloud_machines.json`, polls `read_idle` over mesh on a
/// 60s cadence bound, renews leases before expiry, and drains/sleeps after `mesh.cloud_idle_sleep_minutes`.
pub fn start_idle_driver(state: AppState) -> Option<CloudIdleDriverHandle> {
    if state
        .svc
        .try_ext::<codeterm_mesh::cloud_ingress::CloudIngressHandle>()
        .is_some()
    {
        return None;
    }
    let controller = state.svc.cloud_machines();
    let provisioning = resolve_provisioning(&state);
    let peer_control = Arc::new(MeshPeerControlAdapter::new(state.clone()));
    let config = state.config.clone();
    let idle_window_ms = Arc::new(move || {
        let minutes = config.lock().mesh.cloud_idle_sleep_minutes;
        let effective = if minutes == 0 {
            DEFAULT_IDLE_SLEEP_MINUTES
        } else {
            minutes
        };
        u64::from(effective).saturating_mul(60_000)
    });
    let ceiling_config = state.config.clone();
    let max_awake_without_owner_ms = Arc::new(move || {
        let minutes = ceiling_config
            .lock()
            .mesh
            .cloud_max_awake_without_owner_minutes;
        (minutes > 0).then(|| u64::from(minutes).saturating_mul(60_000))
    });
    let handle = spawn_cloud_idle_driver(
        controller,
        provisioning,
        peer_control,
        Arc::new(TokioIdleTickWaiter),
        CloudIdleDriverConfig::with_idle_window_reader(idle_window_ms)
            .with_max_awake_without_owner_reader(max_awake_without_owner_ms),
    );
    let watcher = spawn_wake_delivery_watcher_with_shutdown(state, Some(handle.subscribe_shutdown()));
    handle.attach_auxiliary_task(watcher);
    Some(handle)
}

/// Event-driven watcher that re-attempts flushing held cloud-wake sends when the guest
/// actually links (at most once per link generation) and settles overdue held sends into
/// `expired_not_delivered` at their exact deadline without polling.
pub fn spawn_wake_delivery_watcher(state: AppState) -> tokio::task::JoinHandle<()> {
    spawn_wake_delivery_watcher_with_shutdown(state, None)
}

async fn wait_for_shutdown(shutdown_rx: &mut Option<tokio::sync::watch::Receiver<bool>>) {
    match shutdown_rx.as_mut() {
        Some(rx) => {
            while !*rx.borrow() {
                if rx.changed().await.is_err() {
                    return;
                }
            }
        }
        None => std::future::pending().await,
    }
}

fn spawn_wake_delivery_watcher_with_shutdown(
    state: AppState,
    mut shutdown_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use codeterm_service_core::service::cloud_machines::CloudPeerControlPort;
        let controller = state.svc.cloud_machines();
        let peer_control = MeshPeerControlAdapter::new(state.clone());
        let mut gate = state.mesh.lock().gate_watch();
        let mut wake_queue_rx = controller.subscribe_wake_queue();
        let mut lifecycle_rx = controller.subscribe_lifecycle();
        loop {
            if shutdown_rx.as_ref().is_some_and(|rx| *rx.borrow()) {
                break;
            }
            gate.borrow_and_update();
            wake_queue_rx.borrow_and_update();
            lifecycle_rx.borrow_and_update();

            controller.expire_all_overdue_held_sends();
            for machine_id in controller.machines_with_held_sends() {
                let Some(record) = controller.find_by_peer(&machine_id) else {
                    continue;
                };
                if let Some(link_generation) = peer_control.current_link_generation(&record) {
                    controller
                        .flush_held_sends_for_link_generation(
                            &record.machine_id,
                            &link_generation,
                            &peer_control,
                        )
                        .await;
                }
            }

            let next_deadline_ms = controller.earliest_held_send_deadline_ms();
            match next_deadline_ms {
                Some(deadline_ms) => {
                    let now_ms = controller.now_ms();
                    let remaining =
                        std::time::Duration::from_millis(deadline_ms.saturating_sub(now_ms));
                    tokio::select! {
                        biased;
                        _ = wait_for_shutdown(&mut shutdown_rx) => break,
                        changed = gate.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                        changed = wake_queue_rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                        changed = lifecycle_rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                        _ = tokio::time::sleep(remaining) => {}
                    }
                }
                None => {
                    tokio::select! {
                        biased;
                        _ = wait_for_shutdown(&mut shutdown_rx) => break,
                        changed = gate.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                        changed = wake_queue_rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                        changed = lifecycle_rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    })
}

pub(crate) fn error_status(error: &CloudError) -> StatusCode {
    match error {
        CloudError::LimitReached { .. }
        | CloudError::ImplicitCreateRefused
        | CloudError::CheckoutTerritory => StatusCode::FORBIDDEN,
        CloudError::LeaseRequired
        | CloudError::CreditsExhausted
        | CloudError::MachineOutOfCredits { .. } => StatusCode::PAYMENT_REQUIRED,
        CloudError::MachineNotFound(_) => StatusCode::NOT_FOUND,
        CloudError::MachineAmbiguous(_)
        | CloudError::MachineBusy { .. }
        | CloudError::InvalidTransition { .. }
        | CloudError::DeleteConfirmationRequired { .. }
        | CloudError::VolumeUpgradeConfirmationRequired { .. }
        | CloudError::SendAlreadyClaimed(_)
        | CloudError::Conflict(_)
        | CloudError::PromoExhausted
        | CloudError::PromoSpendCapExceeded => StatusCode::CONFLICT,
        CloudError::DiskFullLocal { .. } => StatusCode::INSUFFICIENT_STORAGE,
        CloudError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
        CloudError::MachineSleeping { .. }
        | CloudError::ProviderUnavailable(_)
        | CloudError::ProviderUnconfirmed(_)
        | CloudError::WakeDeadlineExceeded { .. }
        | CloudError::ApplyUnconfirmed { .. }
        | CloudError::CheckoutUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        CloudError::ProviderRefused(_)
        | CloudError::PromoInvalidCode
        | CloudError::PromoExpired
        | CloudError::PromoDisabled
        | CloudError::CheckoutInvalid
        | CloudError::CheckoutBelowMin
        | CloudError::CheckoutAboveMax => StatusCode::BAD_REQUEST,
        CloudError::EnrollmentFailed(_) | CloudError::Internal(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub(super) fn error_response(error: CloudError) -> (StatusCode, Json<serde_json::Value>) {
    let status = error_status(&error);
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
    if let CloudError::RateLimited {
        retry_after_seconds,
    } = &error
    {
        body["error"]["retry_after_seconds"] = serde_json::json!(retry_after_seconds);
    }
    (status, Json(body))
}

pub(super) fn require_local_caller(
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

pub(super) fn require_local_owner(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(
        crate::auth::authenticated_request(state, headers),
        Ok((
            crate::auth::CallerIdentity::Local,
            crate::auth::AuthenticatedOrigin::Owner,
        ))
    ) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "local_owner_only",
                    "message": "Cloud credit checkout requires the local owner",
                }
            })),
        ))
    }
}

pub(crate) async fn check_credits_for_wake(
    state: &AppState,
    machine_id: &str,
) -> Result<(), CloudError> {
    let provisioning = resolve_provisioning(state);
    state
        .svc
        .cloud_machines()
        .refresh_credits_for_wake(machine_id, provisioning.as_ref())
        .await?;
    Ok(())
}

pub(crate) async fn wake_machine_for_use(
    state: &AppState,
    machine_id: &str,
) -> Result<codeterm_service_core::service::cloud_machines::CloudReceipt, CloudError> {
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(machine_id)
        .ok_or_else(|| CloudError::MachineNotFound(machine_id.to_owned()))?;
    let provisioning = resolve_provisioning(state);
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let is_cold = matches!(
        record
            .ownership
            .auxiliary_resource_ids
            .get(AUX_KEY_STORAGE_TIER)
            .map(String::as_str),
        Some("cold" | "recoverable_deleted")
    );
    if is_cold {
        let enrollment = BakerEnrollmentAdapter {
            svc: state.svc.clone(),
        };
        return ctrl
            .recreate_from_archive(
                &record.machine_id,
                None,
                provisioning.as_ref(),
                &enrollment,
                &peer_ctrl,
                DEFAULT_WAKE_HOLD_DEADLINE_MS,
            )
            .await;
    }
    ctrl.wake(
        &record.machine_id,
        provisioning.as_ref(),
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
    pub size: Option<String>,
    pub name: Option<String>,
    pub retry_machine_id: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

async fn create_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateCloudBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let provisioning = resolve_provisioning(&state);
    let enrollment = BakerEnrollmentAdapter {
        svc: state.svc.clone(),
    };
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let receipt = state
        .svc
        .cloud_machines()
        .create(
            CreateCloudMachineParams {
                size: body.size.unwrap_or_else(|| "small".into()),
                name: body.name,
                retry_machine_id: body.retry_machine_id,
                extra_labels: body.labels,
            },
            provisioning.as_ref(),
            &enrollment,
            &peer_ctrl,
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
    Ok(Json(serde_json::json!({
        "machines": machines,
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
    let provisioning = resolve_provisioning(&state);
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let receipt = ctrl
        .sleep(&record.machine_id, provisioning.as_ref(), &peer_ctrl, true)
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

async fn archive_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let provisioning = resolve_provisioning(&state);
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let receipt = ctrl
        .archive_now(&record.machine_id, provisioning.as_ref(), &peer_ctrl)
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
    #[serde(default)]
    pub mode: Option<CloudDeleteMode>,
    #[serde(default)]
    pub archive_id: Option<String>,
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
    let provisioning = resolve_provisioning(&state);
    let mode = body.mode.unwrap_or(CloudDeleteMode::Permanent);
    let receipt = ctrl
        .delete_with_mode(
            &record.machine_id,
            mode,
            body.archive_id.as_deref(),
            body.confirm,
            provisioning.as_ref(),
        )
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub struct RecreateCloudBody {
    #[serde(default)]
    pub archive_id: Option<String>,
}

async fn recreate_machine(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RecreateCloudBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let provisioning = resolve_provisioning(&state);
    let enrollment = BakerEnrollmentAdapter {
        svc: state.svc.clone(),
    };
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let receipt = ctrl
        .recreate_from_archive(
            &id,
            body.archive_id.as_deref(),
            provisioning.as_ref(),
            &enrollment,
            &peer_ctrl,
            DEFAULT_WAKE_HOLD_DEADLINE_MS,
        )
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeVolumeBody {
    pub new_size_gb: u32,
    #[serde(default)]
    pub confirm: bool,
}

async fn resize_volume(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ResizeVolumeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_owner(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let provisioning = resolve_provisioning(&state);
    let reply = ctrl
        .resize_volume(
            &record.machine_id,
            body.new_size_gb,
            body.confirm,
            provisioning.as_ref(),
        )
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(reply).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub struct ResolveCloudBody {
    #[serde(default)]
    pub target: String,
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
    let provisioning = resolve_provisioning(&state);
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let enrollment = BakerEnrollmentAdapter {
        svc: state.svc.clone(),
    };
    let create_params = matches!(target, CloudTarget::New).then(|| CreateCloudMachineParams {
        size: body.size.unwrap_or_else(|| "small".into()),
        name: body.name,
        retry_machine_id: None,
        extra_labels: BTreeMap::new(),
    });
    let receipt = ctrl
        .resolve_placement(
            &target,
            create_params,
            provisioning.as_ref(),
            &enrollment,
            &peer_ctrl,
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
    let provisioning = resolve_provisioning(&state);
    let summary = state
        .svc
        .cloud_machines()
        .usage(provisioning.as_ref())
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(summary).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedeemPromoBody {
    pub code: String,
}

async fn redeem_promo(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RedeemPromoBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_caller(&state, &headers)?;
    let code = body.code.trim();
    if code.is_empty() || code.len() > 64 {
        return Err(error_response(CloudError::PromoInvalidCode));
    }
    let provisioning = resolve_provisioning(&state);
    let redeemed = state
        .svc
        .cloud_machines()
        .redeem_promo(code, provisioning.as_ref())
        .await
        .map_err(error_response)?;
    if !redeemed.already_redeemed {
        state
            .svc
            .record_product_event(codeterm_analytics::Event::PromoRedeemed {});
    }
    Ok(Json(serde_json::to_value(redeemed).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudCheckoutBody {
    pub pack_id: String,
    #[serde(default)]
    pub amount_cents: Option<u64>,
}

async fn create_checkout(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CloudCheckoutBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_local_owner(&state, &headers)?;
    let pack_id = body.pack_id.trim();
    if pack_id.is_empty() {
        return Err(error_response(CloudError::CheckoutInvalid));
    }
    if pack_id == "custom" {
        if !body.amount_cents.is_some_and(|cents| cents > 0) {
            return Err(error_response(CloudError::CheckoutInvalid));
        }
    } else if body.amount_cents.is_some() {
        return Err(error_response(CloudError::CheckoutInvalid));
    }
    let provisioning = resolve_provisioning(&state);
    let session = provisioning
        .create_checkout(pack_id, body.amount_cents)
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(session).unwrap_or_default()))
}

fn require_peer_control_authority(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    crate::runtime_login::require_permission_authority(state, headers).map_err(
        |(status, message)| {
            (
                status,
                Json(serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "peer_control_forbidden",
                        "message": message,
                    }
                })),
            )
        },
    )
}

fn authorized_viewer_count(state: &AppState) -> usize {
    let mesh = state.mesh.lock();
    crate::stream_census::STREAMS.live_count_matching_peers(|peer_id| {
        mesh.resolve_peer_including_offline(peer_id)
            .is_ok_and(|peer| {
                peer.access_level == codeterm_service_core::mesh::types::AccessLevel::Full
            })
    })
}

async fn peer_idle(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_peer_control_authority(&state, &headers)?;
    let viewers = authorized_viewer_count(&state);
    let snapshot = state.svc.cloud_peer_busy_snapshot(viewers);
    let draining = state.svc.cloud_machines().local_drain_gate().is_draining();
    Ok(Json(serde_json::json!({
        "active_agent_turns": snapshot.active_agent_turns,
        "running_background_jobs": snapshot.running_background_jobs,
        "held_sends": snapshot.held_sends,
        "attached_viewers": snapshot.attached_viewers,
        "idle": snapshot.is_idle(),
        "draining": draining,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerHoursExhaustedBody {
    lease_expires_at_ms: u64,
}

async fn peer_hours_exhausted(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PeerHoursExhaustedBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_peer_control_authority(&state, &headers)?;
    if state
        .svc
        .try_ext::<codeterm_mesh::cloud_ingress::CloudIngressHandle>()
        .is_none()
    {
        return Err(error_response(CloudError::ProviderUnavailable(
            "cloud guest required".into(),
        )));
    }
    state
        .svc
        .show_cloud_hours_exhausted_in_tabs(body.lease_expires_at_ms);
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn peer_drain(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_peer_control_authority(&state, &headers)?;
    let viewers = authorized_viewer_count(&state);
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

async fn peer_reopen(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_peer_control_authority(&state, &headers)?;
    state.svc.cloud_machines().local_drain_gate().reopen();
    Ok(Json(serde_json::json!({ "ok": true, "draining": false })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerExportArchiveBody {
    archive_id: String,
    owner_uid: String,
    machine_id: String,
    generation: u32,
    #[serde(default)]
    revision: Option<u32>,
    upload_url: String,
    #[serde(default)]
    max_archive_bytes: Option<u64>,
}

struct ZeroizedBytes(Vec<u8>);

impl Drop for ZeroizedBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: byte is a valid mutable reference to owned memory in `self.0`.
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// Reads the one-shot 0600 export key staged by `container.py` in `/run/domios-cloud/export-archive.key`,
/// immediately unlinks the file so a second export without a new delivery refuses with `missing_key`,
/// validates it is 64 hex chars, and wraps it in `ZeroizedBytes`.
fn consume_one_shot_export_key(runtime_dir: &std::path::Path) -> Result<ZeroizedBytes, CloudError> {
    let key_path = runtime_dir.join("export-archive.key");
    let raw = std::fs::read(&key_path).map_err(|_| {
        CloudError::ProviderUnavailable("missing_key: one-shot export key not staged".into())
    })?;
    // Unlink immediately after reading so the key is strictly single-use even if validation or export fails.
    let _ = std::fs::remove_file(&key_path);
    let mut raw_guard = ZeroizedBytes(raw);
    let trimmed = std::str::from_utf8(&raw_guard.0)
        .map(str::trim)
        .unwrap_or("");
    let valid_hex = trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid_hex {
        drop(raw_guard);
        return Err(CloudError::ProviderUnavailable(
            "missing_key: invalid one-shot export key format".into(),
        ));
    }
    let cleaned = ZeroizedBytes(trimmed.as_bytes().to_vec());
    raw_guard.0.iter_mut().for_each(|b| *b = 0);
    drop(raw_guard);
    Ok(cleaned)
}

/// Authenticated controller/Noise peer endpoint invoked during hot->cold archival.
/// Enters drain first (refusing while work runs), consumes the one-shot 0600 export key from
/// tmpfs (`/run/domios-cloud/export-archive.key`), unlinks and zeroizes it, and passes request
/// metadata + key strictly on stdin (never in env or argv) to `cloud_archive.py export-and-upload`.
async fn peer_export_archive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PeerExportArchiveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_peer_control_authority(&state, &headers)?;
    let viewers = authorized_viewer_count(&state);
    let snapshot = state.svc.cloud_peer_busy_snapshot(viewers);
    let now_ms = state.svc.cloud_machines().now_ms();
    let local_id = state.mesh.lock().status().instance_id;
    state
        .svc
        .cloud_machines()
        .local_drain_gate()
        .try_enter_drain(&local_id, snapshot, now_ms)
        .map_err(error_response)?;

    let runtime_dir = std::path::PathBuf::from(
        std::env::var("DOMIOS_CLOUD_RUNTIME_DIR").unwrap_or_else(|_| "/run/domios-cloud".into()),
    );
    let key_guard = match consume_one_shot_export_key(&runtime_dir) {
        Ok(key) => key,
        Err(error) => {
            state.svc.cloud_machines().local_drain_gate().reopen();
            return Err(error_response(error));
        }
    };

    let script_path = std::env::var("DOMIOS_CLOUD_ARCHIVE_SCRIPT")
        .unwrap_or_else(|_| "/usr/local/lib/domios-cloud-archive.py".into());
    let python_bin = std::env::var("DOMIOS_CLOUD_PYTHON").unwrap_or_else(|_| "python3".into());
    let key_str = std::str::from_utf8(&key_guard.0).unwrap_or("");
    let stdin_payload = ZeroizedBytes(
        serde_json::json!({
            "archive_id": body.archive_id,
            "owner_uid": body.owner_uid,
            "machine_id": body.machine_id,
            "generation": body.generation,
            "revision": body.revision.unwrap_or(1),
            "upload_url": body.upload_url,
            "max_archive_bytes": body.max_archive_bytes.unwrap_or(512 * 1024 * 1024),
            "owner_key_hex": key_str,
        })
        .to_string()
        .into_bytes(),
    );
    drop(key_guard);

    let run_result = async {
        use tokio::io::AsyncWriteExt;
        let mut child = tokio::process::Command::new(&python_bin)
            .arg(&script_path)
            .arg("export-and-upload")
            .env_remove("DOMIOS_EXPORT_ARCHIVE_KEY_HEX")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| CloudError::Internal(format!("spawn archive helper: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&stdin_payload.0)
                .await
                .map_err(|e| CloudError::Internal(format!("write archive stdin: {e}")))?;
        }
        drop(stdin_payload);
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| CloudError::Internal(format!("wait archive helper: {e}")))?;
        if !output.status.success() {
            let err_text = String::from_utf8_lossy(&output.stderr);
            return Err(CloudError::ProviderUnavailable(format!(
                "guest archive export refused: {}",
                err_text.trim()
            )));
        }
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .map_err(|e| CloudError::Internal(format!("parse archive receipt: {e}")))
    }
    .await;

    match run_result {
        Ok(receipt) => Ok(Json(receipt)),
        Err(error) => {
            // Export failed: reopen drain so hot machine remains usable and live volume stays intact.
            state.svc.cloud_machines().local_drain_gate().reopen();
            Err(error_response(error))
        }
    }
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/mesh/cloud/create", post(create_machine))
        .route("/api/mesh/cloud/list", get(list_machines))
        .route("/api/mesh/cloud/{id}/sleep", post(sleep_machine))
        .route("/api/mesh/cloud/{id}/archive", post(archive_machine))
        .route("/api/mesh/cloud/{id}/wake", post(wake_machine))
        .route("/api/mesh/cloud/{id}/delete", post(delete_machine))
        .route("/api/mesh/cloud/{id}/recreate", post(recreate_machine))
        .route("/api/mesh/cloud/{id}/resize-volume", post(resize_volume))
        .route(
            "/api/mesh/cloud/{id}/bootstrap",
            get(warm_bootstrap::machine_bootstrap_status),
        )
        .route(
            "/api/mesh/cloud/{id}/provider-signin/prepare",
            post(warm_bootstrap::prepare_machine_provider_signin),
        )
        .route(
            "/api/mesh/cloud/{id}/provider-signin/timing",
            post(warm_bootstrap::record_provider_signin_timing),
        )
        .route("/api/mesh/cloud/resolve", post(resolve_machine))
        .route("/api/mesh/cloud/usage", get(usage_summary))
        .route("/api/mesh/cloud/redeem", post(redeem_promo))
        .route("/api/mesh/cloud/checkout", post(create_checkout))
        .route("/api/mesh/cloud/peer/idle", get(peer_idle))
        .route("/api/mesh/cloud/peer/drain", post(peer_drain))
        .route(
            "/api/mesh/cloud/peer/hours-exhausted",
            post(peer_hours_exhausted),
        )
        .route("/api/mesh/cloud/peer/reopen", post(peer_reopen))
        .route(
            "/api/mesh/cloud/peer/export-archive",
            post(peer_export_archive),
        )
        .route(
            "/api/mesh/cloud/peer/warm-bootstrap",
            post(warm_bootstrap::peer_warm_bootstrap),
        )
        .route(
            "/api/mesh/cloud/peer/provider-readiness",
            get(warm_bootstrap::peer_provider_readiness),
        )
        .route(
            "/api/mesh/cloud/peer/provider-signin/prepare",
            post(warm_bootstrap::peer_prepare_provider_signin),
        )
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod warm_bootstrap_tests;
