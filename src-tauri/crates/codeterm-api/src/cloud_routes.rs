mod adapters;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_service_core::service::cloud_machines::{
    spawn_cloud_idle_driver, CloudError, CloudIdleDriverConfig, CloudIdleDriverHandle,
    CloudProvisioningPort, CloudTarget, CreateCloudMachineParams, TokioIdleTickWaiter,
    DEFAULT_IDLE_SLEEP_MINUTES, DEFAULT_WAKE_HOLD_DEADLINE_MS,
};
use serde::Deserialize;

use crate::routes::AppState;

pub use adapters::MeshPeerControlAdapter;
use adapters::{AccountCloudProvisioningAdapter, BakerEnrollmentAdapter};
#[cfg(any(test, feature = "test-support"))]
pub use adapters::{CloudEndpointPublisherOverride, CloudProvisioningOverride};

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
    Some(spawn_cloud_idle_driver(
        controller,
        provisioning,
        peer_control,
        Arc::new(TokioIdleTickWaiter),
        CloudIdleDriverConfig::with_idle_window_reader(idle_window_ms)
            .with_max_awake_without_owner_reader(max_awake_without_owner_ms),
    ))
}

fn error_response(error: CloudError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match &error {
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
        | CloudError::SendAlreadyClaimed(_)
        | CloudError::Conflict(_)
        | CloudError::PromoExhausted
        | CloudError::PromoSpendCapExceeded => StatusCode::CONFLICT,
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
    if let CloudError::RateLimited {
        retry_after_seconds,
    } = &error
    {
        body["error"]["retry_after_seconds"] = serde_json::json!(retry_after_seconds);
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

fn require_local_owner(
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
    let provisioning = resolve_provisioning(&state);
    let receipt = ctrl
        .delete(&record.machine_id, body.confirm, provisioning.as_ref())
        .await
        .map_err(error_response)?;
    Ok(Json(serde_json::to_value(receipt).unwrap_or_default()))
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
    let redeemed = provisioning
        .redeem_promo(code)
        .await
        .map_err(error_response)?;
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

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/mesh/cloud/create", post(create_machine))
        .route("/api/mesh/cloud/list", get(list_machines))
        .route("/api/mesh/cloud/{id}/sleep", post(sleep_machine))
        .route("/api/mesh/cloud/{id}/wake", post(wake_machine))
        .route("/api/mesh/cloud/{id}/delete", post(delete_machine))
        .route("/api/mesh/cloud/resolve", post(resolve_machine))
        .route("/api/mesh/cloud/usage", get(usage_summary))
        .route("/api/mesh/cloud/redeem", post(redeem_promo))
        .route("/api/mesh/cloud/checkout", post(create_checkout))
        .route("/api/mesh/cloud/peer/idle", get(peer_idle))
        .route("/api/mesh/cloud/peer/drain", post(peer_drain))
        .route("/api/mesh/cloud/peer/reopen", post(peer_reopen))
}

#[cfg(test)]
mod tests;
