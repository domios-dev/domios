//! High-level peer management (CLI-friendly): connect / approve / reject / disconnect.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::{future::Future, sync::Arc};

use crate::routes::{check_auth, AppState};

#[cfg(test)]
mod pairing_correlation_tests;
#[cfg(test)]
mod reachability_tests;

mod delivery;
mod manual;
use delivery::PairingSendDisposition;
pub use manual::mesh_pair_identity;

#[derive(Debug, PartialEq)]
pub enum ConnectPeerResult {
    Pending { request_id: String, existing: bool },
    Connected,
}

impl ConnectPeerResult {
    pub fn as_json(&self) -> serde_json::Value {
        match self {
            Self::Pending {
                request_id,
                existing,
            } => serde_json::json!({
                "ok": true,
                "status": "pending",
                "request_id": request_id,
                "existing": existing,
            }),
            Self::Connected => serde_json::json!({"ok": true, "status": "connected"}),
        }
    }
}

#[derive(Debug)]
pub enum ConnectPeerError {
    NotFound(String),
    Conflict(String),
    BadRequest(String),
    Internal(String),
    BeforeSend(String),
    Refused(String),
    Delivery(String),
}

impl std::fmt::Display for ConnectPeerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message)
            | Self::Conflict(message)
            | Self::BadRequest(message)
            | Self::Internal(message)
            | Self::BeforeSend(message)
            | Self::Refused(message)
            | Self::Delivery(message) => f.write_str(message),
        }
    }
}

impl ConnectPeerError {
    fn pairing_send_disposition(&self) -> Option<PairingSendDisposition> {
        match self {
            Self::BeforeSend(_) => Some(PairingSendDisposition::DefinitelyBeforeSend),
            Self::Refused(_) => Some(PairingSendDisposition::Terminal),
            Self::Delivery(_) => Some(PairingSendDisposition::Uncertain),
            Self::NotFound(_) | Self::Conflict(_) | Self::BadRequest(_) | Self::Internal(_) => None,
        }
    }
}

fn existing_connect_result(
    mesh: &codeterm_service_core::mesh::MeshManager,
    instance_id: &str,
) -> Option<ConnectPeerResult> {
    if mesh.paired_peers.contains_key(instance_id) {
        return Some(ConnectPeerResult::Connected);
    }

    mesh.pairing_targets
        .iter()
        .find_map(|(request_id, (_, _, target_id))| {
            (target_id == instance_id)
                .then(|| mesh.pending_requests.get(request_id))
                .flatten()
                .filter(|request| {
                    request.status == codeterm_service_core::mesh::types::PairingStatus::Pending
                })
                .map(|request| ConnectPeerResult::Pending {
                    request_id: request.request_id.clone(),
                    existing: true,
                })
        })
}

enum ConnectPreparation {
    Existing(ConnectPeerResult),
    Send {
        request: codeterm_service_core::mesh::types::PairingRequest,
        address: String,
        port: u16,
        delivery: Arc<codeterm_service_core::mesh::PairingDelivery>,
    },
    Wait(Arc<codeterm_service_core::mesh::PairingDelivery>),
}

struct ConnectOwner {
    mesh: codeterm_service_core::MeshStateHandle,
    target_id: String,
    request_id: String,
    delivery: Arc<codeterm_service_core::mesh::PairingDelivery>,
    completed: bool,
}

impl ConnectOwner {
    fn complete(&mut self, outcome: Result<(), ConnectPeerError>) -> Result<(), String> {
        let terminal = complete_connect(
            &self.mesh,
            &self.target_id,
            &self.request_id,
            &self.delivery,
            outcome,
        );
        self.completed = true;
        terminal
    }
}

impl Drop for ConnectOwner {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.complete(Err(ConnectPeerError::Delivery(
                "phase=unknown;source_depth=0;causes=none".to_string(),
            )));
        }
    }
}

#[cfg(test)]
fn prepare_connect(
    mesh: &mut codeterm_service_core::mesh::MeshManager,
    instance_id: &str,
) -> Result<ConnectPreparation, ConnectPeerError> {
    prepare_connect_at(mesh, instance_id, None)
}

fn prepare_connect_at(
    mesh: &mut codeterm_service_core::mesh::MeshManager,
    instance_id: &str,
    endpoint: Option<std::net::SocketAddr>,
) -> Result<ConnectPreparation, ConnectPeerError> {
    mesh.cleanup_expired_requests();
    let peer = codeterm_service_core::mesh::discovered::resolve_discovered(
        &mesh.discovered_peers,
        instance_id,
    )
    .map_err(|error| match error {
        codeterm_service_core::mesh::discovered::ResolveError::NotFound => {
            ConnectPeerError::NotFound(error.to_string())
        }
        codeterm_service_core::mesh::discovered::ResolveError::Ambiguous(_) => {
            ConnectPeerError::Conflict(error.to_string())
        }
    })?;
    let target_id = peer.instance_id.clone();
    if mesh.paired_peers.contains_key(&target_id) {
        return Ok(ConnectPreparation::Existing(ConnectPeerResult::Connected));
    }
    if let Some(delivery) = mesh.pairing_deliveries.get(&target_id) {
        return Ok(ConnectPreparation::Wait(delivery.clone()));
    }
    if let Some(result) = existing_connect_result(mesh, &target_id) {
        return Ok(ConnectPreparation::Existing(result));
    }
    if mesh.pairing_targets.len() >= codeterm_service_core::mesh::MAX_PAIRING_DELIVERIES
        || mesh.pairing_deliveries.len() >= codeterm_service_core::mesh::MAX_PAIRING_DELIVERIES
    {
        return Err(ConnectPeerError::Conflict(
            "Pairing delivery capacity reached; cancel or await expiry".to_string(),
        ));
    }
    let address = peer
        .addresses
        .iter()
        .find(|address| address.is_tailscale)
        .or_else(|| peer.addresses.first())
        .ok_or_else(|| ConnectPeerError::BadRequest("No address for peer".to_string()))?;
    let public_key = mesh
        .ident
        .identity
        .public_key
        .clone()
        .ok_or_else(|| ConnectPeerError::Internal("No identity keypair".to_string()))?;
    let from_addresses = codeterm_service_core::mesh::discovery::remote_addresses(mesh)
        .map_err(ConnectPeerError::BadRequest)?;
    let request = codeterm_service_core::mesh::types::PairingRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        from_instance_id: mesh.ident.identity.instance_id.clone(),
        from_instance_name: mesh.ident.instance_name.clone(),
        from_address: from_addresses
            .iter()
            .find(|address| address.is_tailscale)
            .unwrap_or(&from_addresses[0])
            .ip
            .clone(),
        from_port: mesh.api_port,
        from_addresses,
        public_key,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        status: codeterm_service_core::mesh::types::PairingStatus::Pending,
        introduced_by: None,
    };
    let address = endpoint
        .map(|e| e.ip().to_string())
        .unwrap_or_else(|| address.ip.clone());
    let port = endpoint.map(|e| e.port()).unwrap_or(peer.api_port);
    mesh.pairing_targets.insert(
        request.request_id.clone(),
        (address.clone(), port, peer.instance_id.clone()),
    );
    mesh.pending_requests
        .insert(request.request_id.clone(), request.clone());
    let delivery = Arc::new(codeterm_service_core::mesh::PairingDelivery::new(
        target_id.clone(),
        request.request_id.clone(),
    ));
    mesh.pairing_deliveries.insert(target_id, delivery.clone());
    Ok(ConnectPreparation::Send {
        request,
        address,
        port,
        delivery,
    })
}

fn complete_connect(
    mesh: &codeterm_service_core::MeshStateHandle,
    instance_id: &str,
    request_id: &str,
    delivery: &Arc<codeterm_service_core::mesh::PairingDelivery>,
    outcome: Result<(), ConnectPeerError>,
) -> Result<(), String> {
    let uncertain = outcome.as_ref().is_err_and(|error| {
        error.pairing_send_disposition() == Some(PairingSendDisposition::Uncertain)
    });
    let outcome = outcome.map_err(|error| error.to_string());
    let mut manager = mesh.lock();
    if uncertain {
        if !delivery.retain() {
            return delivery.outcome();
        }
        drop(manager);
        delivery.publish();
        return outcome;
    }
    if !delivery.claim(outcome.clone()) {
        return delivery.outcome();
    }
    if outcome.is_err() {
        manager.pending_requests.remove(request_id);
        manager.pairing_targets.remove(request_id);
    }
    if manager
        .pairing_deliveries
        .get(instance_id)
        .is_some_and(|current| Arc::ptr_eq(current, delivery))
    {
        manager.pairing_deliveries.remove(instance_id);
    }
    drop(manager);
    delivery.publish();
    outcome
}

#[cfg(test)]
pub async fn connect_peer_with_sender<F, Fut>(
    mesh: codeterm_service_core::MeshStateHandle,
    instance_id: String,
    sender: F,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(codeterm_service_core::mesh::types::PairingRequest, String, u16) -> Fut,
    Fut: Future<Output = Result<String, ConnectPeerError>>,
{
    connect_peer_with_sender_and_waiter(mesh, instance_id, sender, || {}).await
}

#[cfg(test)]
async fn connect_peer_with_sender_and_waiter<F, Fut, W>(
    mesh: codeterm_service_core::MeshStateHandle,
    instance_id: String,
    sender: F,
    waiter: W,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(codeterm_service_core::mesh::types::PairingRequest, String, u16) -> Fut,
    Fut: Future<Output = Result<String, ConnectPeerError>>,
    W: FnOnce(),
{
    connect_peer_with_sender_and_waiter_and_confirmation(
        mesh,
        instance_id,
        sender,
        waiter,
        || async {},
    )
    .await
}

#[cfg(test)]
async fn connect_peer_with_sender_and_waiter_and_confirmation<F, Fut, W, C, CFut>(
    mesh: codeterm_service_core::MeshStateHandle,
    instance_id: String,
    sender: F,
    waiter: W,
    confirmed: C,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(codeterm_service_core::mesh::types::PairingRequest, String, u16) -> Fut,
    Fut: Future<Output = Result<String, ConnectPeerError>>,
    W: FnOnce(),
    C: FnOnce() -> CFut,
    CFut: Future<Output = ()>,
{
    connect_peer_with_endpoint(mesh, instance_id, None, sender, waiter, confirmed).await
}

async fn connect_peer_with_endpoint<F, Fut, W, C, CFut>(
    mesh: codeterm_service_core::MeshStateHandle,
    instance_id: String,
    endpoint: Option<std::net::SocketAddr>,
    sender: F,
    waiter: W,
    confirmed: C,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(codeterm_service_core::mesh::types::PairingRequest, String, u16) -> Fut,
    Fut: Future<Output = Result<String, ConnectPeerError>>,
    W: FnOnce(),
    C: FnOnce() -> CFut,
    CFut: Future<Output = ()>,
{
    let preparation = {
        let mut manager = mesh.lock();
        prepare_connect_at(&mut manager, &instance_id, endpoint)?
    };
    let (request, address, port, delivery) = match preparation {
        ConnectPreparation::Existing(result) => return Ok(result),
        ConnectPreparation::Wait(delivery) => {
            waiter();
            delivery.wait().await.map_err(ConnectPeerError::Delivery)?;
            return Ok(ConnectPeerResult::Pending {
                request_id: delivery.request_id().to_string(),
                existing: true,
            });
        }
        ConnectPreparation::Send {
            request,
            address,
            port,
            delivery,
        } => (request, address, port, delivery),
    };
    let mut owner = ConnectOwner {
        mesh: mesh.clone(),
        target_id: delivery.target_id().to_string(),
        request_id: request.request_id.clone(),
        delivery,
        completed: false,
    };
    let remote_instance_id = match sender(request.clone(), address, port).await {
        Ok(id) => id,
        Err(error) => {
            let message = error.to_string();
            let terminal = owner.complete(Err(error));
            return Err(ConnectPeerError::Delivery(match terminal {
                Ok(()) => message,
                Err(error) => error,
            }));
        }
    };
    let confirmation = {
        let mut manager = mesh.lock();
        manager.confirm_pairing_target_identity(&request.request_id, &remote_instance_id)
    };
    if let Err(error) = confirmation {
        let terminal = owner.complete(Err(ConnectPeerError::Refused(error.clone())));
        return Err(ConnectPeerError::Delivery(match terminal {
            Ok(()) => error,
            Err(error) => error,
        }));
    }
    confirmed().await;
    if let Err(error) = owner.complete(Ok(())) {
        return Err(ConnectPeerError::Delivery(error));
    }
    Ok(ConnectPeerResult::Pending {
        request_id: request.request_id,
        existing: false,
    })
}

pub async fn connect_peer(
    mesh: codeterm_service_core::MeshStateHandle,
    instance_id: String,
) -> Result<ConnectPeerResult, ConnectPeerError> {
    connect_peer_at(mesh, instance_id, None).await
}

async fn connect_peer_at(
    mesh: codeterm_service_core::MeshStateHandle,
    instance_id: String,
    endpoint: Option<std::net::SocketAddr>,
) -> Result<ConnectPeerResult, ConnectPeerError> {
    connect_peer_with_endpoint(
        mesh,
        instance_id,
        endpoint,
        |request, address, port| async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .map_err(|error| ConnectPeerError::BeforeSend(error.to_string()))?;
            let endpoint = std::net::SocketAddr::new(
                address
                    .parse()
                    .map_err(|_| ConnectPeerError::BadRequest("Invalid peer IP".into()))?,
                port,
            );
            let response = client
                .post(format!("http://{endpoint}/api/mesh/pair/request"))
                .json(&request)
                .send()
                .await
                .map_err(|error| {
                    let detail = delivery::safe_reqwest_detail(&error);
                    match delivery::classify_reqwest_error(
                        error.is_timeout(),
                        error.is_connect(),
                        error.is_body() || error.is_decode(),
                    ) {
                        PairingSendDisposition::DefinitelyBeforeSend => {
                            ConnectPeerError::BeforeSend(detail)
                        }
                        PairingSendDisposition::Uncertain | PairingSendDisposition::Terminal => {
                            ConnectPeerError::Delivery(detail)
                        }
                    }
                })?;
            if !response.status().is_success() {
                return Err(ConnectPeerError::Refused(format!(
                    "Pair request rejected with HTTP {}",
                    response.status()
                )));
            }
            response
                .json::<serde_json::Value>()
                .await
                .map_err(|error| {
                    ConnectPeerError::Delivery(delivery::safe_reqwest_detail(&error).to_string())
                })?["instance_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| {
                    ConnectPeerError::Delivery(
                        "Pair response omitted the peer identity".to_string(),
                    )
                })
        },
        || {},
        || async {},
    )
    .await
}

/// Initiate pairing with a discovered peer (same logic as Tauri mesh_connect).
pub async fn mesh_connect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let instance_id = body["instance_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    let result = manual::connect_from_body(state.mesh.clone(), instance_id, &body)
        .await
        .map_err(rest_connect_error)?;
    crate::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(result.as_json()))
}

fn rest_connect_error(error: ConnectPeerError) -> (StatusCode, String) {
    let status = match error {
        ConnectPeerError::NotFound(_) => StatusCode::NOT_FOUND,
        ConnectPeerError::Conflict(_) => StatusCode::CONFLICT,
        ConnectPeerError::BadRequest(_) => StatusCode::BAD_REQUEST,
        ConnectPeerError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ConnectPeerError::BeforeSend(_)
        | ConnectPeerError::Refused(_)
        | ConnectPeerError::Delivery(_) => StatusCode::BAD_GATEWAY,
    };
    (status, error.to_string())
}

#[cfg(test)]
mod tests;

/// Approve an incoming pairing request (same logic as Tauri mesh_approve_pairing).
pub async fn mesh_approve_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let request_id = body["request_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    use crate::mesh_routes::pairing::{do_approve, ApproveError};
    do_approve(
        state.mesh.clone(),
        state.svc.app_handle.clone(),
        request_id.clone(),
    )
    .await
    .map_err(|e| {
        let status = match &e {
            ApproveError::NotFound => StatusCode::NOT_FOUND,
            ApproveError::Expired => StatusCode::GONE,
            ApproveError::NoKeypair => StatusCode::INTERNAL_SERVER_ERROR,
            ApproveError::Network(_) | ApproveError::Rejected(_) => StatusCode::BAD_GATEWAY,
        };
        (status, e.to_string())
    })?;
    log::info!("[mesh/api] Pairing approved for request {}", request_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Reject and block an incoming pairing request (same logic as Tauri mesh_reject_pairing).
pub async fn mesh_reject_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let request_id = body["request_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    let mut mesh = state.mesh.lock();
    reject_pending_request_http(&mut mesh, &request_id)?;
    drop(mesh);

    crate::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true})))
}

fn reject_pending_request_http(
    mesh: &mut codeterm_service_core::mesh::MeshManager,
    request_id: &str,
) -> Result<(), (StatusCode, String)> {
    crate::mesh_routes::pairing::reject_pending_request(mesh, request_id)
        .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))
}

/// Disconnect (unpair) from a paired peer (same logic as Tauri mesh_disconnect).
pub async fn mesh_disconnect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let arg = body["instance_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    let instance_id = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&arg).cloned().or_else(|| {
            m.paired_peers
                .values()
                .find(|p| p.instance_name == arg)
                .cloned()
        });
        peer.map(|p| p.instance_id).unwrap_or(arg)
    };

    if !state.mesh.lock().paired_peers.contains_key(&instance_id) {
        return Err((
            StatusCode::NOT_FOUND,
            format!("No paired peer matches '{}'", instance_id),
        ));
    }

    state.mesh.lock().unpair(&instance_id);

    crate::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    log::info!("[mesh/api] Disconnected from {}", instance_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_approve_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_approve_peer(
        state,
        headers,
        Json(serde_json::json!({"request_id": request_id})),
    )
    .await
}

pub async fn mesh_reject_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_reject_peer(
        state,
        headers,
        Json(serde_json::json!({"request_id": request_id})),
    )
    .await
}

pub async fn mesh_disconnect_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_disconnect_peer(
        state,
        headers,
        Json(serde_json::json!({"instance_id": instance_id})),
    )
    .await
}
