//! REST endpoints for owner-submitted secure input requests.
//!
//! - `POST /api/secure-input/requests`: Local Owner or authorized local Tab creates a request.
//! - `GET  /api/secure-input/requests`: Local Owner lists pending/recent requests.
//! - `GET  /api/secure-input/requests/{id}`: Local Owner or creating Tab reads typed status only.
//! - `POST /api/secure-input/requests/{id}/submit`: Strictly Local Owner submits value once.
//! - `POST /api/secure-input/requests/{id}/cancel`: Local Owner or creating Tab cancels request.
//! - `POST /api/secure-input/deliver`: Target machine accepts Local Owner or Full-access Peer
//!   with Owner origin and routes the value directly to its declared consumer.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_service_core::{
    mesh::types::{AccessLevel, RelayResponse},
    service::{
        agent_runtime::login::SecretString,
        secure_input::{
            self, ConsumerParseOptions, SecureInputConsumer, SecureInputCreateParams,
            SecureInputDeliveryPayload, SecureInputDeliveryResult, SecureInputError,
            SecureInputKind, SecureInputRequestView, SecureInputStatus,
        },
    },
};
use serde::Deserialize;

use crate::{
    auth::{authenticated_request, AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/secure-input/requests",
            get(list_requests).post(create_request),
        )
        .route("/api/secure-input/requests/{id}", get(get_request))
        .route(
            "/api/secure-input/requests/{id}/submit",
            post(submit_request),
        )
        .route(
            "/api/secure-input/requests/{id}/cancel",
            post(cancel_request),
        )
        .route("/api/secure-input/deliver", post(deliver_secret))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRequestBody {
    #[serde(default)]
    machine: Option<String>,
    target: String,
    kind: SecureInputKind,
    reason: String,
    #[serde(default)]
    overwrite: bool,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    login_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    epoch: Option<u64>,
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    method_id: Option<String>,
    #[serde(default)]
    ttl_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmitRequestBody {
    value: SecretString,
    #[serde(default)]
    expected_installation_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelRequestBody {
    #[serde(default)]
    reason: Option<String>,
}

fn no_store_json<T: serde::Serialize>(value: T) -> (HeaderMap, Json<T>) {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (headers, Json(value))
}

fn map_service_error(err: SecureInputError) -> (StatusCode, String) {
    let status = match err {
        SecureInputError::BadRequest(_) => StatusCode::BAD_REQUEST,
        SecureInputError::NotFound(_) => StatusCode::NOT_FOUND,
        SecureInputError::Forbidden(_) => StatusCode::FORBIDDEN,
        SecureInputError::Conflict(_) => StatusCode::CONFLICT,
    };
    (status, err.message().to_owned())
}

fn require_local_owner(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    match authenticated_request(state, headers)? {
        (CallerIdentity::Local, AuthenticatedOrigin::Owner) => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Only the local human owner may perform this secure input operation".to_owned(),
        )),
    }
}

/// Returns `Ok(is_remote_delivery)` on valid Owner authority.
fn require_delivery_authority(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<bool, (StatusCode, String)> {
    match authenticated_request(state, headers)? {
        (CallerIdentity::Local, AuthenticatedOrigin::Owner) => Ok(false),
        (
            CallerIdentity::Peer {
                access_level: AccessLevel::Full,
                ..
            },
            AuthenticatedOrigin::Owner,
        ) => Ok(true),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Secure input delivery requires the local owner or a Full-access peer with Owner origin"
                .to_owned(),
        )),
    }
}

struct ResolvedTargetMachine {
    installation_id: String,
    machine_name: String,
    is_remote: bool,
}

fn resolve_target_machine(
    state: &AppState,
    requested_machine: Option<&str>,
) -> Result<ResolvedTargetMachine, (StatusCode, String)> {
    let (own_id, own_name, peer_name_collision) = {
        let mesh = state.mesh.lock();
        let status = mesh.status();
        let selector = requested_machine.map(str::trim).filter(|s| !s.is_empty());
        let collides_with_peer = selector.is_some_and(|sel| {
            mesh.paired_peers.values().any(|peer| {
                peer.instance_name.eq_ignore_ascii_case(sel)
                    || peer.instance_id.eq_ignore_ascii_case(sel)
            })
        });
        (status.instance_id, status.instance_name, collides_with_peer)
    };
    let selector = requested_machine.map(str::trim).filter(|s| !s.is_empty());

    let Some(selector) = selector else {
        return Ok(ResolvedTargetMachine {
            installation_id: own_id,
            machine_name: own_name,
            is_remote: false,
        });
    };
    if selector == own_id {
        return Ok(ResolvedTargetMachine {
            installation_id: own_id,
            machine_name: own_name,
            is_remote: false,
        });
    }
    if selector.eq_ignore_ascii_case("local") || selector.eq_ignore_ascii_case(&own_name) {
        if peer_name_collision {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "Target machine '{selector}' matches both the local host and a paired peer; specify the exact installation id"
                ),
            ));
        }
        return Ok(ResolvedTargetMachine {
            installation_id: own_id,
            machine_name: own_name,
            is_remote: false,
        });
    }

    let (peer_id, peer_name) =
        crate::mesh_routes::relay::resolve_secure_input_peer(state, selector)?;
    Ok(ResolvedTargetMachine {
        installation_id: peer_id,
        machine_name: peer_name,
        is_remote: true,
    })
}

fn validate_tab_requester(
    state: &AppState,
    tab_id: &str,
    is_remote: bool,
    consumer: &SecureInputConsumer,
) -> Result<(), (StatusCode, String)> {
    let mux = state.svc.mux.read();
    let Some(tab) = mux.get_tab(tab_id) else {
        return Err((
            StatusCode::FORBIDDEN,
            "Requesting tab is no longer registered".to_owned(),
        ));
    };
    if tab.neighbor_authority.is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "Neighbor tabs cannot create owner secure input requests".to_owned(),
        ));
    }
    if is_remote && tab.restorable_launch.execution_sandbox.is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "Sandboxed agent tabs cannot request secure input for remote machines".to_owned(),
        ));
    }
    if !is_remote {
        let target_tab = match consumer {
            SecureInputConsumer::ProviderLogin { tab_id, .. } => Some(tab_id.as_str()),
            SecureInputConsumer::ProviderKey { tab_id, .. } => tab_id.as_deref(),
            SecureInputConsumer::VaultSlot { .. } => None,
        };
        if let Some(target_tab) = target_tab {
            let allowed_cross_tab = tab_id == target_tab
                || codeterm_service_core::service::general_agent::identity::is_general_agent(tab)
                || (tab.role
                    == Some(codeterm_service_core::multiplexer::types::AgentRole::Manager)
                    && tab.role_profile.as_deref() == Some("orchestrator"));
            if !allowed_cross_tab {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Only the owning tab, General Agent, or Orchestrator may target another local tab for sign-in or provider key input"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

async fn create_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequestBody>,
) -> Result<(HeaderMap, Json<SecureInputRequestView>), (StatusCode, String)> {
    let requester_tab_id = match authenticated_request(&state, &headers)? {
        (CallerIdentity::Local, AuthenticatedOrigin::Owner) => None,
        (CallerIdentity::Tab(caller), _) => Some(caller.tab_id),
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                "Secure input requests may only be created locally by the owner or an authorized local agent tab"
                    .to_owned(),
            ));
        }
    };

    let consumer = SecureInputConsumer::parse_target(
        &body.target,
        ConsumerParseOptions {
            overwrite: body.overwrite,
            provider_id: body.provider_id,
            login_id: body.login_id,
            session_id: body.session_id,
            nonce: body.nonce,
            epoch: body.epoch,
            tab_id: body.tab_id,
            method_id: body.method_id,
        },
    )
    .map_err(|reason| (StatusCode::BAD_REQUEST, reason))?;

    let target_machine = resolve_target_machine(&state, body.machine.as_deref())?;
    if let Some(tab_id) = requester_tab_id.as_deref() {
        validate_tab_requester(&state, tab_id, target_machine.is_remote, &consumer)?;
    }

    let view = state
        .svc
        .secure_input()
        .create(
            SecureInputCreateParams {
                target_installation_id: target_machine.installation_id,
                target_machine_name: target_machine.machine_name,
                is_remote: target_machine.is_remote,
                consumer,
                kind: body.kind,
                reason: body.reason,
                requester_tab_id,
                ttl_secs: body.ttl_secs,
            },
            secure_input::now_ms(),
        )
        .map_err(map_service_error)?;

    secure_input::emit_requests_changed(&state.svc);
    Ok(no_store_json(view))
}

async fn list_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, String)> {
    require_local_owner(&state, &headers)?;
    let requests = state.svc.secure_input().list(secure_input::now_ms());
    Ok(no_store_json(serde_json::json!({ "requests": requests })))
}

async fn get_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(HeaderMap, Json<SecureInputRequestView>), (StatusCode, String)> {
    let caller = authenticated_request(&state, &headers)?;
    let view = state
        .svc
        .secure_input()
        .get(&id, secure_input::now_ms())
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Secure input request '{id}' not found"),
            )
        })?;
    match caller {
        (CallerIdentity::Local, AuthenticatedOrigin::Owner) => Ok(no_store_json(view)),
        (CallerIdentity::Tab(caller), _)
            if view.requester_tab_id.as_deref() == Some(caller.tab_id.as_str()) =>
        {
            Ok(no_store_json(view))
        }
        _ => Err((
            StatusCode::FORBIDDEN,
            "Only the owner or the requesting tab may read this secure input request".to_owned(),
        )),
    }
}

async fn cancel_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<CancelRequestBody>>,
) -> Result<(HeaderMap, Json<SecureInputRequestView>), (StatusCode, String)> {
    let (is_owner, requester_tab) = match authenticated_request(&state, &headers)? {
        (CallerIdentity::Local, AuthenticatedOrigin::Owner) => (true, None),
        (CallerIdentity::Tab(caller), _) => (false, Some(caller.tab_id)),
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                "Only the owner or the requesting tab may cancel this secure input request"
                    .to_owned(),
            ));
        }
    };
    let detail = body.and_then(|Json(b)| b.reason);
    let view = state
        .svc
        .secure_input()
        .cancel(
            &id,
            requester_tab.as_deref(),
            is_owner,
            detail.as_deref(),
            secure_input::now_ms(),
        )
        .map_err(map_service_error)?;
    secure_input::emit_requests_changed(&state.svc);
    Ok(no_store_json(view))
}

async fn submit_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SubmitRequestBody>,
) -> Result<(HeaderMap, Json<SecureInputRequestView>), (StatusCode, String)> {
    require_local_owner(&state, &headers)?;
    if body.value.as_str().trim().is_empty()
        || body.value.as_str().len() > secure_input::MAX_VALUE_BYTES
        || body.value.as_str().contains('\0')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Secret value must be non-empty, within size limits, and free of NUL bytes".to_owned(),
        ));
    }

    let now = secure_input::now_ms();
    let mut claimed = state
        .svc
        .secure_input()
        .claim_submission(&id, body.expected_installation_id.as_deref(), now)
        .map_err(map_service_error)?;
    // Publish `Submitting` state immediately so any concurrent UI refresh stays locked.
    secure_input::emit_requests_changed(&state.svc);

    let delivery = if !claimed.is_remote {
        let own_id = state.mesh.lock().status().instance_id;
        secure_input::deliver_to_consumer(
            &state.svc,
            &own_id,
            false,
            SecureInputDeliveryPayload {
                request_id: claimed.request_id.clone(),
                target_installation_id: claimed.target_installation_id.clone(),
                consumer: claimed.consumer.clone(),
                kind: claimed.kind,
                expires_at_ms: claimed.expires_at_ms,
                value: body.value,
            },
            secure_input::now_ms(),
        )
        .await
    } else {
        submit_remote_delivery(&state, &headers, &claimed, body.value).await
    };

    let updated = state
        .svc
        .secure_input()
        .settle_submission(&id, delivery.status, delivery.reason.clone())
        .unwrap_or_else(|| {
            claimed.status = delivery.status;
            claimed.reason_detail = delivery.reason.map(|r| secure_input::sanitize_detail(&r));
            claimed
        });
    secure_input::emit_requests_changed(&state.svc);
    Ok(no_store_json(updated))
}

async fn submit_remote_delivery(
    state: &AppState,
    headers: &HeaderMap,
    claimed: &SecureInputRequestView,
    value: SecretString,
) -> SecureInputDeliveryResult {
    // Re-verify peer Full access and live traffic before transmitting any secret bytes.
    let authorized = {
        let mesh = state.mesh.lock();
        mesh.paired_peers
            .get(&claimed.target_installation_id)
            .is_some_and(|peer| {
                peer.access_level == AccessLevel::Full && mesh.traffic_open(&peer.instance_id)
            })
    };
    if !authorized {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some(
                "Target machine no longer has active Full access; nothing was sent".to_owned(),
            ),
        };
    }

    let mut payload_json = serde_json::json!({
        "requestId": claimed.request_id,
        "targetInstallationId": claimed.target_installation_id,
        "consumer": claimed.consumer,
        "kind": claimed.kind,
        "expiresAtMs": claimed.expires_at_ms,
        "value": value.as_str(),
    });
    drop(value);

    let relay_result = crate::mesh_routes::relay::send_owner_secure_delivery(
        state,
        headers,
        &claimed.target_installation_id,
        &claimed.request_id,
        &mut payload_json,
    )
    .await;
    secure_input::zeroize_secure_input_json_value(&mut payload_json);
    classify_remote_delivery_result(relay_result)
}

pub(crate) fn classify_remote_delivery_result(
    relay_result: Result<RelayResponse, RelayResponse>,
) -> SecureInputDeliveryResult {
    match relay_result {
        Ok(resp) if resp.ok => {
            if let Some(data) = resp.data {
                if let Ok(parsed) = serde_json::from_value::<SecureInputDeliveryResult>(data) {
                    return parsed;
                }
            }
            SecureInputDeliveryResult {
                status: SecureInputStatus::Delivered,
                reason: None,
            }
        }
        Ok(resp) => {
            let proven_unwritten = relay_response_bytes_written(&resp) == Some(false)
                || matches!(resp.status, 400 | 401 | 403 | 404);
            let status = if proven_unwritten {
                SecureInputStatus::Refused
            } else {
                SecureInputStatus::Uncertain
            };
            SecureInputDeliveryResult {
                status,
                reason: resp
                    .error
                    .or_else(|| Some("Remote machine refused secure input delivery".to_owned())),
            }
        }
        Err(resp) => {
            let status = if relay_response_bytes_written(&resp) == Some(false) {
                SecureInputStatus::Refused
            } else {
                SecureInputStatus::Uncertain
            };
            SecureInputDeliveryResult {
                status,
                reason: resp
                    .error
                    .or_else(|| Some("Secure input delivery outcome was uncertain".to_owned())),
            }
        }
    }
}

/// Extracts `data["error"]["bytes_written"]` as populated by `relay_failure()`.
/// Absent or `Some(true)` is never treated as "nothing written".
fn relay_response_bytes_written(resp: &RelayResponse) -> Option<bool> {
    resp.data
        .as_ref()?
        .pointer("/error/bytes_written")?
        .as_bool()
}

async fn deliver_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SecureInputDeliveryPayload>,
) -> Result<(HeaderMap, Json<SecureInputDeliveryResult>), (StatusCode, String)> {
    let is_remote = require_delivery_authority(&state, &headers)?;
    let own_id = state.mesh.lock().status().instance_id;
    let result = secure_input::deliver_to_consumer(
        &state.svc,
        &own_id,
        is_remote,
        payload,
        secure_input::now_ms(),
    )
    .await;
    Ok(no_store_json(result))
}

#[cfg(test)]
mod tests;
