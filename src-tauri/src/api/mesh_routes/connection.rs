//! High-level peer management (CLI-friendly): connect / approve / reject / disconnect.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::{future::Future, sync::Arc};

use crate::api::routes::{check_auth, AppState};

#[cfg(test)]
mod reachability_tests;
#[cfg(test)]
mod pairing_correlation_tests;

mod delivery;
use delivery::PairingSendDisposition;

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
            Self::NotFound(_)
            | Self::Conflict(_)
            | Self::BadRequest(_)
            | Self::Internal(_) => None,
        }
    }
}

fn existing_connect_result(
    mesh: &crate::mesh::MeshManager,
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
                .filter(|request| request.status == crate::mesh::types::PairingStatus::Pending)
                .map(|request| ConnectPeerResult::Pending {
                    request_id: request.request_id.clone(),
                    existing: true,
                })
        })
}

enum ConnectPreparation {
    Existing(ConnectPeerResult),
    Send {
        request: crate::mesh::types::PairingRequest,
        address: String,
        port: u16,
        delivery: Arc<crate::mesh::PairingDelivery>,
    },
    Wait(Arc<crate::mesh::PairingDelivery>),
}

struct ConnectOwner {
    mesh: crate::MeshStateHandle,
    target_id: String,
    request_id: String,
    delivery: Arc<crate::mesh::PairingDelivery>,
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

fn prepare_connect(
    mesh: &mut crate::mesh::MeshManager,
    instance_id: &str,
) -> Result<ConnectPreparation, ConnectPeerError> {
    mesh.cleanup_expired_requests();
    let peer = crate::mesh::discovered::resolve_discovered(&mesh.discovered_peers, instance_id)
        .map_err(|error| match error {
            crate::mesh::discovered::ResolveError::NotFound => {
                ConnectPeerError::NotFound(error.to_string())
            }
            crate::mesh::discovered::ResolveError::Ambiguous(_) => {
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
    if mesh.pairing_targets.len() >= crate::mesh::MAX_PAIRING_DELIVERIES
        || mesh.pairing_deliveries.len() >= crate::mesh::MAX_PAIRING_DELIVERIES
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
    let from_addresses =
        crate::mesh::discovery::remote_addresses(mesh).map_err(ConnectPeerError::BadRequest)?;
    let request = crate::mesh::types::PairingRequest {
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
        status: crate::mesh::types::PairingStatus::Pending,
        introduced_by: None,
    };
    let address = address.ip.clone();
    let port = peer.api_port;
    mesh.pairing_targets.insert(
        request.request_id.clone(),
        (address.clone(), port, peer.instance_id.clone()),
    );
    mesh.pending_requests
        .insert(request.request_id.clone(), request.clone());
    let delivery = Arc::new(crate::mesh::PairingDelivery::new(
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
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    request_id: &str,
    delivery: &Arc<crate::mesh::PairingDelivery>,
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

pub async fn connect_peer_with_sender<F, Fut>(
    mesh: crate::MeshStateHandle,
    instance_id: String,
    sender: F,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(crate::mesh::types::PairingRequest, String, u16) -> Fut,
    Fut: Future<Output = Result<String, ConnectPeerError>>,
{
    connect_peer_with_sender_and_waiter(mesh, instance_id, sender, || {}).await
}

async fn connect_peer_with_sender_and_waiter<F, Fut, W>(
    mesh: crate::MeshStateHandle,
    instance_id: String,
    sender: F,
    waiter: W,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(crate::mesh::types::PairingRequest, String, u16) -> Fut,
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

async fn connect_peer_with_sender_and_waiter_and_confirmation<F, Fut, W, C, CFut>(
    mesh: crate::MeshStateHandle,
    instance_id: String,
    sender: F,
    waiter: W,
    confirmed: C,
) -> Result<ConnectPeerResult, ConnectPeerError>
where
    F: FnOnce(crate::mesh::types::PairingRequest, String, u16) -> Fut,
    Fut: Future<Output = Result<String, ConnectPeerError>>,
    W: FnOnce(),
    C: FnOnce() -> CFut,
    CFut: Future<Output = ()>,
{
    let preparation = {
        let mut manager = mesh.lock();
        prepare_connect(&mut manager, &instance_id)?
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
    mesh: crate::MeshStateHandle,
    instance_id: String,
) -> Result<ConnectPeerResult, ConnectPeerError> {
    connect_peer_with_sender(mesh, instance_id, |request, address, port| async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        let response = client
            .post(format!("http://{}:{}/api/mesh/pair/request", address, port))
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                let detail = delivery::safe_reqwest_detail(&error);
                match delivery::classify_reqwest_error(error.is_timeout(), error.is_connect()) {
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
            .map_err(|error| ConnectPeerError::Delivery(format!(
                "{}",
                delivery::safe_reqwest_detail(&error)
            )))?["instance_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                ConnectPeerError::Delivery("Pair response omitted the peer identity".to_string())
            })
    })
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

    let result = connect_peer(state.mesh.clone(), instance_id)
        .await
        .map_err(rest_connect_error)?;
    crate::api::events::emit_everywhere(
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
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    const PEER_ID: &str = "31c7cff6-4bd2-43b0-a5cd-b5db9bfbb673";
    const PEER_NAME: &str = "peer";

    fn mesh_state() -> crate::MeshStateHandle {
        let mut manager = crate::mesh::MeshManager::new("test".to_string(), 7685);
        manager.ready_listeners = vec!["100.64.0.1:7685".parse().unwrap()];
        manager.disk_persist = false;
        manager.ident.identity.public_key = Some("key".to_string());
        manager.discovered_peers.insert(
            PEER_ID.to_string(),
            crate::mesh::types::DiscoveredPeer {
                instance_id: PEER_ID.to_string(),
                instance_name: PEER_NAME.to_string(),
                addresses: vec![crate::mesh::types::PeerAddress::from_ip("127.0.0.1")],
                api_port: 7685,
                discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
                last_seen: 0,
                dev: true,
                mesh_id: None,
                vouched_by: vec![],
            },
        );
        Arc::new(parking_lot::Mutex::new(manager))
    }

    #[tokio::test]
    async fn mesh_connect_peer_first_send_and_duplicate_return_the_same_pending_request() {
        let mesh = mesh_state();
        let first = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
            Ok(PEER_ID.to_string())
        })
        .await
        .unwrap();
        let ConnectPeerResult::Pending {
            request_id,
            existing,
        } = first
        else {
            panic!("pending result expected")
        };
        assert!(!existing);

        let duplicate =
            connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
                panic!("duplicate must not send")
            })
            .await
            .unwrap();
        assert_eq!(
            duplicate,
            ConnectPeerResult::Pending {
                request_id: request_id.clone(),
                existing: true
            }
        );
        assert_eq!(mesh.lock().pending_requests.len(), 1);
    }

    async fn assert_aliases_share_one_delivery<M>(
        first_target: &str,
        duplicate_target: &str,
        mutate_while_waiting: M,
    ) where
        M: FnOnce(&crate::MeshStateHandle),
    {
        let mesh = mesh_state();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let first_mesh = mesh.clone();
        let first_target = first_target.to_string();
        let first = tokio::spawn(async move {
            connect_peer_with_sender(first_mesh, first_target, |_, _, _| async move {
                started_tx.send(()).expect("first delivery starts");
                release_rx.await.expect("test releases first delivery");
                Ok(PEER_ID.to_string())
            })
            .await
        });
        started_rx.await.expect("first delivery starts");

        let duplicate_mesh = mesh.clone();
        let duplicate_target = duplicate_target.to_string();
        let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
        let duplicate = tokio::spawn(async move {
            connect_peer_with_sender_and_waiter(
                duplicate_mesh,
                duplicate_target,
                |_, _, _| async { panic!("alias duplicate must not send") },
                || wait_tx.send(()).expect("alias duplicate waits"),
            )
            .await
        });
        wait_rx.await.expect("alias duplicate waits");
        mutate_while_waiting(&mesh);
        release_tx.send(()).expect("release first delivery");

        let first = first.await.unwrap().unwrap();
        let duplicate = duplicate.await.unwrap().unwrap();
        let ConnectPeerResult::Pending {
            request_id,
            existing: false,
        } = first
        else {
            panic!("first request creates pending pairing")
        };
        assert_eq!(
            duplicate,
            ConnectPeerResult::Pending {
                request_id,
                existing: true,
            }
        );
    }

    #[tokio::test]
    async fn mesh_connect_peer_name_then_uuid_share_the_same_delivery() {
        assert_aliases_share_one_delivery(PEER_NAME, PEER_ID, |_| {}).await;
    }

    #[tokio::test]
    async fn mesh_connect_peer_uuid_then_name_share_the_same_delivery() {
        assert_aliases_share_one_delivery(PEER_ID, PEER_NAME, |_| {}).await;
    }

    #[tokio::test]
    async fn mesh_connect_peer_waiter_survives_discovery_removal() {
        assert_aliases_share_one_delivery(PEER_ID, PEER_NAME, |mesh| {
            mesh.lock().discovered_peers.remove(PEER_ID);
        })
        .await;
    }

    #[tokio::test]
    async fn mesh_connect_peer_waiter_survives_discovery_identity_rekey() {
        assert_aliases_share_one_delivery(PEER_ID, PEER_NAME, |mesh| {
            let mut manager = mesh.lock();
            let mut peer = manager
                .discovered_peers
                .remove(PEER_ID)
                .expect("test peer exists");
            peer.instance_id = "bffac0f8-3d21-4e8a-aa52-c59ef82eb80f".to_string();
            peer.instance_name = "renamed peer".to_string();
            manager
                .discovered_peers
                .insert(peer.instance_id.clone(), peer);
        })
        .await;
    }

    #[tokio::test]
    async fn mesh_connect_peer_same_name_success_returns_existing_pending() {
        let mesh = mesh_state();
        let first =
            connect_peer_with_sender(mesh.clone(), PEER_NAME.to_string(), |_, _, _| async {
                Ok(PEER_ID.to_string())
            })
            .await
            .unwrap();
        let duplicate = connect_peer_with_sender(mesh, PEER_NAME.to_string(), |_, _, _| async {
            panic!("same-name duplicate must not send")
        })
        .await
        .unwrap();
        assert!(matches!(
            first,
            ConnectPeerResult::Pending {
                existing: false,
                ..
            }
        ));
        assert!(matches!(
            duplicate,
            ConnectPeerResult::Pending { existing: true, .. }
        ));
    }

    #[tokio::test]
    async fn mesh_connect_peer_already_connected_returns_connected_without_sending() {
        let mesh = mesh_state();
        {
            let mut manager = mesh.lock();
            manager.paired_peers.insert(
                PEER_ID.to_string(),
                crate::mesh::types::PairedPeer {
                    instance_id: PEER_ID.to_string(),
                    instance_name: "peer".to_string(),
                    addresses: vec![],
                    api_port: 7685,
                    public_key: "key".to_string(),
                    paired_at: "now".to_string(),
                    last_seen: None,
                    online: true,
                    is_primary: false,
                    access_level: crate::mesh::types::AccessLevel::Full,
                    version: None,
                    web_ui: None,
                    relay_protocol_version: 0,
                    consecutive_misses: 0,
                    last_active_ms: None,
                },
            );
        }
        let result = connect_peer_with_sender(mesh, PEER_ID.to_string(), |_, _, _| async {
            panic!("paired peer must not send")
        })
        .await
        .unwrap();
        assert_eq!(result, ConnectPeerResult::Connected);
    }

    #[tokio::test]
    async fn mesh_connect_peer_definitely_before_send_rolls_back_so_retry_can_send() {
        let mesh = mesh_state();
        let error = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
            Err(ConnectPeerError::BeforeSend("offline".to_string()))
        })
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "offline");
        assert!(mesh.lock().pending_requests.is_empty());
        assert!(mesh.lock().pairing_targets.is_empty());

        let retry = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
            Ok(PEER_ID.to_string())
        })
        .await
        .unwrap();
        assert!(matches!(
            retry,
            ConnectPeerResult::Pending {
                existing: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn mesh_connect_peer_duplicate_waits_for_before_send_failure_and_retry_is_fresh() {
        let mesh = mesh_state();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let first_mesh = mesh.clone();
        let first = tokio::spawn(async move {
            connect_peer_with_sender(first_mesh, PEER_ID.to_string(), |_, _, _| async move {
                started_tx
                    .send(())
                    .expect("test waits for the first delivery");
                release_rx.await.expect("test releases the first delivery");
                Err(ConnectPeerError::BeforeSend("offline".to_string()))
            })
            .await
        });
        started_rx.await.expect("first delivery starts");

        let duplicate_mesh = mesh.clone();
        let (duplicate_wait_tx, duplicate_wait_rx) = tokio::sync::oneshot::channel();
        let duplicate = tokio::spawn(async move {
            connect_peer_with_sender_and_waiter(
                duplicate_mesh,
                PEER_ID.to_string(),
                |_, _, _| async { panic!("duplicate must wait for the in-flight delivery") },
                || {
                    duplicate_wait_tx
                        .send(())
                        .expect("duplicate starts waiting")
                },
            )
            .await
        });
        duplicate_wait_rx
            .await
            .expect("duplicate waits for first delivery");
        release_tx.send(()).expect("release first delivery");

        assert_eq!(first.await.unwrap().unwrap_err().to_string(), "offline");
        assert_eq!(duplicate.await.unwrap().unwrap_err().to_string(), "offline");
        assert!(mesh.lock().pending_requests.is_empty());
        assert!(mesh.lock().pairing_targets.is_empty());

        let sends = Arc::new(AtomicUsize::new(0));
        let retry_sends = sends.clone();
        let retry = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), move |_, _, _| {
            retry_sends.fetch_add(1, Ordering::SeqCst);
            async { Ok(PEER_ID.to_string()) }
        })
        .await
        .unwrap();
        assert!(matches!(
            retry,
            ConnectPeerResult::Pending {
                existing: false,
                ..
            }
        ));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        assert_eq!(mesh.lock().pending_requests.len(), 1);
    }

    #[tokio::test]
    async fn mesh_connect_peer_duplicate_waits_for_identity_mismatch_and_retry_is_fresh() {
        let mesh = mesh_state();
        mesh.lock()
            .discovered_peers
            .get_mut(PEER_ID)
            .expect("test peer exists")
            .discovery_method = crate::mesh::types::DiscoveryMethod::Udp;
        let wrong_id = "01d1dc9d-fcf2-49a5-8a02-0a1fc681b29d".to_string();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let first_mesh = mesh.clone();
        let first = tokio::spawn(async move {
            connect_peer_with_sender(first_mesh, PEER_ID.to_string(), move |_, _, _| async move {
                started_tx
                    .send(())
                    .expect("test waits for the first delivery");
                release_rx.await.expect("test releases the first delivery");
                Ok(wrong_id)
            })
            .await
        });
        started_rx.await.expect("first delivery starts");

        let duplicate_mesh = mesh.clone();
        let (duplicate_wait_tx, duplicate_wait_rx) = tokio::sync::oneshot::channel();
        let duplicate = tokio::spawn(async move {
            connect_peer_with_sender_and_waiter(
                duplicate_mesh,
                PEER_ID.to_string(),
                |_, _, _| async { panic!("duplicate must wait for identity confirmation") },
                || {
                    duplicate_wait_tx
                        .send(())
                        .expect("duplicate starts waiting")
                },
            )
            .await
        });
        duplicate_wait_rx
            .await
            .expect("duplicate waits for identity confirmation");
        release_tx.send(()).expect("release first delivery");

        let expected = format!(
            "Peer identity mismatch: expected {PEER_ID}, got 01d1dc9d-fcf2-49a5-8a02-0a1fc681b29d"
        );
        assert_eq!(first.await.unwrap().unwrap_err().to_string(), expected);
        assert_eq!(duplicate.await.unwrap().unwrap_err().to_string(), expected);
        assert!(mesh.lock().pending_requests.is_empty());
        assert!(mesh.lock().pairing_targets.is_empty());

        let sends = Arc::new(AtomicUsize::new(0));
        let retry_sends = sends.clone();
        let retry = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), move |_, _, _| {
            retry_sends.fetch_add(1, Ordering::SeqCst);
            async { Ok(PEER_ID.to_string()) }
        })
        .await
        .unwrap();
        assert!(matches!(
            retry,
            ConnectPeerResult::Pending {
                existing: false,
                ..
            }
        ));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        assert_eq!(mesh.lock().pending_requests.len(), 1);
    }

    #[tokio::test]
    async fn mesh_connect_peer_owner_cancellation_wakes_waiter_and_allows_one_fresh_retry() {
        let mesh = mesh_state();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let first_mesh = mesh.clone();
        let owner = tokio::spawn(async move {
            connect_peer_with_sender(first_mesh, PEER_NAME.to_string(), |_, _, _| async move {
                started_tx.send(()).expect("owner delivery starts");
                std::future::pending::<Result<String, ConnectPeerError>>().await
            })
            .await
        });
        started_rx.await.expect("owner delivery starts");

        let duplicate_mesh = mesh.clone();
        let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(async move {
            connect_peer_with_sender_and_waiter(
                duplicate_mesh,
                PEER_ID.to_string(),
                |_, _, _| async { panic!("waiter must not send") },
                || wait_tx.send(()).expect("waiter starts waiting"),
            )
            .await
        });
        wait_rx.await.expect("waiter starts waiting");
        owner.abort();
        assert!(owner.await.expect_err("owner was aborted").is_cancelled());
        assert!(matches!(
            waiter.await.unwrap().unwrap(),
            ConnectPeerResult::Pending { existing: true, .. }
        ));
        let request_id = mesh
            .lock()
            .pending_requests
            .keys()
            .next()
            .cloned()
            .expect("cancellation retains correlation until disposition");
        assert!(mesh.lock().cancel_outgoing_pairing(&request_id));
        assert!(mesh.lock().pairing_deliveries.is_empty());
        assert!(mesh.lock().pending_requests.is_empty());
        assert!(mesh.lock().pairing_targets.is_empty());

        let sends = Arc::new(AtomicUsize::new(0));
        let retry_sends = sends.clone();
        let retry = connect_peer_with_sender(mesh, PEER_NAME.to_string(), move |_, _, _| {
            retry_sends.fetch_add(1, Ordering::SeqCst);
            async { Ok(PEER_ID.to_string()) }
        })
        .await
        .unwrap();
        assert!(matches!(
            retry,
            ConnectPeerResult::Pending {
                existing: false,
                ..
            }
        ));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mesh_connect_peer_rest_and_tauri_share_the_same_result_mapping() {
        let result = ConnectPeerResult::Pending {
            request_id: "request-1".to_string(),
            existing: true,
        };
        let rest = result.as_json();
        let tauri = result.as_json();
        assert_eq!(rest, tauri);
        assert_eq!(rest["status"], "pending");
        assert_eq!(rest["existing"], true);
    }
}

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

    use crate::api::mesh_routes::pairing::{do_approve, ApproveError};
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
    let block_id = if let Some(req) = mesh.pending_requests.get_mut(&request_id) {
        req.status = crate::mesh::types::PairingStatus::Rejected;
        Some(req.from_instance_id.clone())
    } else {
        None
    };
    if let Some(id) = block_id {
        if !mesh.revocation_list.blocked.contains(&id) {
            mesh.revocation_list.blocked.push(id);
        }
        let _ = mesh.save();
    }
    drop(mesh);

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true})))
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

    // keeps the API forgiving without bloating the CLI.
    let (instance_id, peer, signed_token, our_id) = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&arg).cloned().or_else(|| {
            m.paired_peers
                .values()
                .find(|p| p.instance_name == arg)
                .cloned()
        });
        let resolved_id = peer.as_ref().map(|p| p.instance_id.clone()).unwrap_or(arg);
        let token = m.sign_request().ok();
        let id = m.ident.identity.instance_id.clone();
        (resolved_id, peer, token, id)
    };

    if peer.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("No paired peer matches '{}'", instance_id),
        ));
    }

    if let (Some(peer), Some(token)) = (peer, signed_token.clone()) {
        if !peer.addresses.is_empty() {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            let api_port = peer.api_port;
            let our_id_for_call = our_id.clone();
            let result = crate::mesh::transport::try_peer(&peer.addresses, |addr| {
                let url = format!(
                    "http://{}:{}/api/mesh/peers/{}",
                    addr.ip, api_port, our_id_for_call
                );
                let http = http.clone();
                let token = token.clone();
                async move {
                    match http
                        .delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            crate::mesh::transport::TryNext::Success(())
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            crate::mesh::transport::TryNext::Stop(format!(
                                "HTTP {}: {}",
                                status, body
                            ))
                        }
                        Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                    }
                }
            })
            .await;
            match result {
                Ok(()) => log::info!("[mesh/api] Notified {} about disconnect", instance_id),
                Err(e) => log::warn!(
                    "[mesh/api] Failed to notify {} about disconnect: {}",
                    instance_id,
                    e
                ),
            }
        }
    }

    // Cascade to remaining peers — tell each one we just unpaired the target so their mirror of paired_peers stays consistent.
    let (remaining_peers, broadcast_token) = {
        let m = state.mesh.lock();
        let peers: Vec<_> = m
            .paired_peers
            .values()
            .filter(|p| p.instance_id != instance_id)
            .cloned()
            .collect();
        let token = m.sign_request().ok();
        (peers, token)
    };

    state.mesh.lock().unpair(&instance_id);

    if let Some(token) = broadcast_token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        for peer in &remaining_peers {
            if peer.addresses.is_empty() {
                continue;
            }
            let api_port = peer.api_port;
            let target = instance_id.clone();
            let _ = crate::mesh::transport::try_peer(&peer.addresses, |addr| {
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, api_port, target);
                let http = http.clone();
                let token = token.clone();
                async move {
                    match http
                        .delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            crate::mesh::transport::TryNext::Success(())
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            crate::mesh::transport::TryNext::Stop(format!("HTTP {}", status))
                        }
                        Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                    }
                }
            })
            .await;
        }
    }

    crate::api::events::emit_everywhere(
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
