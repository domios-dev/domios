use crate::routes::AppState;
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State as AxumState},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    Extension, Json,
};
use std::sync::LazyLock;

mod caller;
mod capability;
mod origin;
mod peer_rail;
mod peer_stream;
#[cfg(test)]
use peer_stream::is_streamable_relay_path;
pub use peer_stream::mesh_relay_stream;

mod sync;
pub use sync::mesh_sync;
#[cfg(test)]
use sync::{sync_input_for_access, sync_output_for_access};

/// Verify relay sender: must be paired. Paired = trusted.
pub(super) fn check_mesh_relay_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let peer = check_mesh_signature(state, headers)?;
    if !crate::auth::authenticated_origin(state, headers)?.owner_trusted() {
        return Err((
            StatusCode::FORBIDDEN,
            "An agent relay cannot use peer authority".into(),
        ));
    }
    Ok(peer)
}

pub(super) fn check_mesh_signature(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let (known_keys, our_id) = {
        let mesh = state.mesh.lock();
        let mut known_keys = mesh.known_public_keys();
        if let Some(ref pk) = mesh.ident.identity.public_key {
            known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
        }
        (known_keys, mesh.ident.identity.instance_id.clone())
    };
    let peer_id = codeterm_service_core::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or unknown signature".to_string(),
            )
        })?;
    if peer_id == our_id {
        return Ok("local".to_string());
    }

    let mut mesh = state.mesh.lock();
    if !mesh.paired_peers.contains_key(&peer_id) {
        return Err((
            StatusCode::UNAUTHORIZED,
            format!("Peer {} not paired", peer_id),
        ));
    }
    mesh.check_and_record_nonce(token)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    Ok(peer_id)
}

/// Inject the authenticated originating-peer name into a relayed `/send` body
/// so the forwarded local send handler can build a peer-qualified source
/// prefix. No-op for non-send relays. The peer name overrides any body-supplied
/// value — it is derived from the signed token, not the (spoofable) payload.
fn tag_relay_send_source(
    body: &mut serde_json::Value,
    method: &str,
    path: &str,
    sender_name: &str,
) {
    if !(path.contains("/send") && method.eq_ignore_ascii_case("POST")) {
        return;
    }
    if let Some(obj) = body.as_object_mut() {
        obj.insert("source_peer".into(), serde_json::json!(sender_name));
    }
}

/// Strip a relayed notification's navigation claims and stamp the authenticated
/// peer name in their place. An origin-local tab or workspace id is meaningless
/// here and would render a Show Tab button pointed at an unrelated local tab.
fn tag_relay_notification_source(
    body: &mut serde_json::Value,
    method: &str,
    path: &str,
    sender_name: &str,
) {
    let bare = path.split('?').next().unwrap_or(path);
    if !(bare == "/api/notifications" && method.eq_ignore_ascii_case("POST")) {
        return;
    }
    if let Some(obj) = body.as_object_mut() {
        obj.remove("tab_id");
        obj.remove("workspace_id");
        obj.insert("source_peer".into(), serde_json::json!(sender_name));
    }
}

/// Stamp the authenticated originating-peer name onto a relayed `agent spawn` so
/// the spawned worker gets a durable cross-mesh report target back to the caller.
/// Mirrors [`tag_relay_send_source`]: the peer name comes from the signed token,
/// never the (spoofable) payload, and overrides any body-supplied value.
/// A forwarded `parent_tab_id` is an origin-local hierarchy identity that is
/// invalid as a target-local parent; promote it to the origin report target and
/// strip it so the remote spawn stays a parentless cross-mesh worker.
fn tag_relay_spawn_source(
    body: &mut serde_json::Value,
    method: &str,
    path: &str,
    sender_id: &str,
    sender_name: &str,
) {
    let bare = path.split('?').next().unwrap_or(path);
    if !(bare == "/api/agents/spawn" && method.eq_ignore_ascii_case("POST")) {
        return;
    }
    if let Some(obj) = body.as_object_mut() {
        obj.insert("origin_peer".into(), serde_json::json!(sender_name));
        let supplied = obj
            .remove("ownership")
            .and_then(|value| serde_json::from_value(value).ok());
        let ownership = crate::agent_routes::spawn_ownership::ownership_for_spawning_peer(
            supplied,
            sender_id,
            sender_name,
        );
        if let Ok(ownership) = serde_json::to_value(ownership) {
            obj.insert("ownership".into(), ownership);
        }
        sanitize_cross_peer_parent(obj);
    }
}

/// Origin relay-out: attach controller/origin-Team ownership from the local
/// caller tab; the target boundary stamps peer identity from the signed sender.
fn enrich_spawn_ownership(
    state: &AppState,
    method: &str,
    path: &str,
    body: &mut serde_json::Value,
) {
    let bare = path.split('?').next().unwrap_or(path);
    if !(bare == "/api/agents/spawn" && method.eq_ignore_ascii_case("POST")) {
        return;
    }
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    let caller = obj
        .get("spawned_by_tab_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let Some(caller) = caller else { return };
    let mux = state.svc.mux.read();
    let Some(tab) = mux.get_tab(&caller) else {
        return;
    };
    let team = tab.team_id.as_ref().and_then(|team_id| {
        mux.get_team(team_id)
            .map(|team| serde_json::json!({ "id": team_id, "title": team.name }))
    });
    let mut ownership = serde_json::json!({
        "controller_title": tab.title,
        "controller_tab_id": caller,
        "peer_id": "",
        "peer_name": "",
    });
    if let Some(team) = team {
        ownership["team"] = team;
    }
    obj.insert("ownership".into(), ownership);
}

fn sanitize_cross_peer_parent(obj: &mut serde_json::Map<String, serde_json::Value>) {
    // Only a cross-mesh Worker is parentless on the target; other roles keep
    // their origin-local parent_tab_id.
    let worker = obj
        .get("role")
        .and_then(|v| v.as_str())
        .and_then(|role| codeterm_types::agent::role_kind::RoleName::parse(role).ok())
        .is_some_and(|role| !role.is_manager_name());
    if !worker {
        return;
    }
    let nonempty = |v: Option<&serde_json::Value>| {
        v.and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let parent = nonempty(obj.remove("parent_tab_id").as_ref());
    if let Some(parent) = parent {
        if nonempty(obj.get("report_to")).is_none()
            && nonempty(obj.get("report_target_tab_id")).is_none()
        {
            obj.insert("report_to".into(), serde_json::json!(parent));
        }
    }
}

/// Local-origin relay: the web mirror stays pointed at its own daemon and asks
/// it to proxy a request to a paired peer. Authenticated as a normal local call
/// (the auth middleware gates it); the daemon then rides the existing signed
/// mesh relay to the target peer. Keeps the browser's single reachability
/// assumption (it only ever talks to the daemon that served it).
#[derive(serde::Deserialize)]
pub struct PeerRelayCommand {
    pub target_peer_id: Option<String>,
    pub target_instance_id: Option<String>,
    #[serde(default = "default_relay_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: serde_json::Value,
}

fn default_relay_method() -> String {
    "GET".to_string()
}

fn is_recurring_delayed_relay(method: &str, path: &str, body: &serde_json::Value) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    let is_create = method.eq_ignore_ascii_case("POST")
        && bare.starts_with("/api/tabs/")
        && bare.ends_with("/delayed-messages");
    let is_edit =
        method.eq_ignore_ascii_case("PATCH") && bare.starts_with("/api/delayed-messages/");
    (is_create || is_edit)
        && body
            .get("recurrence")
            .is_some_and(|recurrence| !recurrence.is_null())
}

fn recurrence_capability_refusal(peer_name: &str, version: Option<&str>) -> String {
    format!(
        "Peer '{}' (version {}) does not advertise delayed_recurrence; update the peer or send a one-shot explicitly",
        peer_name,
        version
            .filter(|version| !version.trim().is_empty())
            .unwrap_or("unknown")
    )
}

fn identify_supports_delayed_recurrence(identify: &serde_json::Value) -> bool {
    identify
        .get("features")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some("delayed_recurrence"))
        })
}

fn recurrence_capability_refusal_response(
    peer_name: &str,
    identify: Option<&serde_json::Value>,
    cached_version: Option<&str>,
) -> Option<codeterm_service_core::mesh::types::RelayResponse> {
    if identify.is_some_and(identify_supports_delayed_recurrence) {
        return None;
    }
    let version = identify
        .and_then(|identify| identify.get("version"))
        .and_then(serde_json::Value::as_str)
        .or(cached_version);
    Some(relay_failure(
        StatusCode::UPGRADE_REQUIRED.as_u16(),
        "peer_lacks_delayed_recurrence",
        Some(false),
        recurrence_capability_refusal(peer_name, version),
    ))
}

/// Parse a successful relayed `agent spawn` into a remote-worker registration
/// `(report_to, remote_tab, title, model)`. None unless this was a POST spawn
/// that succeeded, carried a `report_to` target, and produced a tab id.
fn spawn_registration_from_relay(
    method: &str,
    path: &str,
    body: &serde_json::Value,
    resp: &codeterm_service_core::mesh::types::RelayResponse,
) -> Option<(String, String, String, Option<String>)> {
    let bare = path.split('?').next().unwrap_or(path);
    if !(bare == "/api/agents/spawn" && method.eq_ignore_ascii_case("POST") && resp.ok) {
        return None;
    }
    let report_to = body
        .get("report_to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let data = resp.data.as_ref()?;
    let remote_tab = data
        .get("tab_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let title = data
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(remote_tab);
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some((
        report_to.to_string(),
        remote_tab.to_string(),
        title.to_string(),
        model,
    ))
}

pub async fn mesh_relay_to_peer(
    AxumState(state): AxumState<AppState>,
    Extension(identity): Extension<crate::auth::CallerIdentity>,
    headers: HeaderMap,
    Json(mut req): Json<PeerRelayCommand>,
) -> Result<Json<PeerRelayResponse>, (StatusCode, String)> {
    let actor = crate::auth::ActorHeaders::authenticated(
        &identity,
        crate::auth::authenticated_origin(&state, &headers)?,
        headers.clone(),
    );
    if let crate::auth::CallerIdentity::Tab(caller) = &identity {
        capability::authorize(&state, &caller, &req.method, &req.path).await?;
        caller::bind(&caller, &req.method, &req.path, &mut req.body)
            .map_err(|message| (StatusCode::FORBIDDEN, message))?;
    }
    let (client, peer_id, peer_name, peer_version) = {
        let mesh = state.mesh.lock();
        match (&req.target_instance_id, &req.target_peer_id) {
            (Some(id), None) => resolve_exact_relay_peer(&mesh, id)?,
            (None, Some(target)) => resolve_relay_peer(&mesh, target)?,
            _ => return Err((StatusCode::BAD_REQUEST, "Provide one relay target".into())),
        }
    };
    let client = origin::peer_client(&state, &headers, client)?;
    let answered_by = |response| {
        Json(PeerRelayResponse {
            response,
            peer: AnsweringPeer {
                instance_id: peer_id.clone(),
                instance_name: peer_name.clone(),
            },
        })
    };
    if is_recurring_delayed_relay(&req.method, &req.path, &req.body) {
        let identify = client.auth_get_json("/api/identify").await.ok();
        if let Some(refusal) = recurrence_capability_refusal_response(
            &peer_name,
            identify.as_ref(),
            peer_version.as_deref(),
        ) {
            return Ok(answered_by(refusal));
        }
    }
    let mut relay_body = req.body;
    if req.path.split('?').next() == Some("/api/agents/spawn")
        && req.method.eq_ignore_ascii_case("POST")
    {
        let named = |field: &str| relay_body.get(field).and_then(|value| value.as_str());
        codeterm_service_core::service::execution_sandbox_refuse_confined_mesh_delegation(
            &state.svc,
            &[
                actor
                    .as_ref()
                    .ok()
                    .and_then(|actor| crate::tab_routes::caller_tab_header(actor))
                    .as_deref(),
                named("spawned_by_tab_id"),
                named("creator_tab_id"),
                named("parent_tab_id"),
            ],
        )
        .map_err(|error| (StatusCode::FORBIDDEN, error))?;
    }
    codeterm_service_core::service::mesh_artifacts_transfer(
        &state.svc,
        actor
            .as_ref()
            .map(|actor| crate::tab_routes::caller_tab_header(actor))
            .map_err(|(_, message)| message.clone()),
        &client,
        &req.method,
        &req.path,
        &mut relay_body,
    )
    .await
    .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    crate::mesh_attachment::guard_remote_send_on_host(&req.method, &req.path, &relay_body)
        .await
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;
    let delivery_id = relay_send_delivery_id(&req.method, &req.path);
    enrich_spawn_ownership(&state, &req.method, &req.path, &mut relay_body);
    let cmd = codeterm_service_core::mesh::types::RelayCommand {
        method: req.method,
        path: req.path,
        body: relay_body,
        delivery_id,
    };
    // The sender hop's own transport failure classifies into the same
    // relay_failure() vocabulary the receiving hop uses (see relay_error_summary).
    let resp = match client.send_relay(&cmd).await {
        Ok(resp) => resp,
        Err(err) => {
            return Ok(answered_by(relay_failure(
                err.http_status(),
                err.code(),
                err.bytes_written(),
                err.to_string(),
            )));
        }
    };

    // sidebar ghosts it under the report target immediately (no report yet).
    if let Some((report_to, remote_tab, title, model)) =
        spawn_registration_from_relay(&cmd.method, &cmd.path, &cmd.body, &resp)
    {
        if let Ok(orchestrator) = crate::routes::resolve_tab_async(&state, &report_to).await {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            state.svc.remote_workers.record_spawn(
                &orchestrator,
                &peer_name,
                &remote_tab,
                &title,
                model,
                now_ms,
            );
            state.svc.emit_remote_workers_changed(&orchestrator);
        }
    }

    Ok(answered_by(resp))
}

pub async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<codeterm_service_core::mesh::types::RelayCommand>,
) -> Result<Json<codeterm_service_core::mesh::types::RelayResponse>, (StatusCode, String)> {
    let sender_id = check_mesh_signature(&state, &headers)?;
    let origin = crate::auth::authenticated_origin(&state, &headers)?;
    let agent = crate::auth::authenticated_agent(&state, &headers)?;

    let Some(delivery_id) = cmd
        .delivery_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && is_tab_send(&cmd.method, &cmd.path))
    else {
        return perform_relay(
            &state,
            &sender_id,
            &cmd,
            origin,
            agent.as_ref(),
            headers.get(header::AUTHORIZATION),
        )
        .await
        .map(Json);
    };

    // At-most-once (#240): one dispatch per (authenticated sender, delivery_id);
    // duplicates replay; a saturated in-flight window is rejected, never grown.
    let agent_tab = agent
        .as_ref()
        .map(|agent| agent.tab_id.as_str())
        .unwrap_or_default();
    let key =
        super::send_dedupe::dedupe_key(&format!("{sender_id}:{origin:?}:{agent_tab}"), delivery_id);
    let entry = match super::send_dedupe::MeshDeliveryDedupe::global()
        .acquire(key.clone(), super::send_dedupe::now_ms())
    {
        super::send_dedupe::Admission::Duplicate(entry) => {
            log::info!(
                "[mesh/relay] dedup replay {} {} from {} (delivery_id={})",
                cmd.method,
                cmd.path,
                sender_id,
                delivery_id
            );
            let response = entry.wait().await;
            return Ok(Json(response));
        }
        super::send_dedupe::Admission::AtCapacity => {
            log::warn!(
                "[mesh/relay] delivery window saturated; rejecting {} {} from {} (delivery_id={})",
                cmd.method,
                cmd.path,
                sender_id,
                delivery_id
            );
            return Ok(Json(saturated_response()));
        }
        super::send_dedupe::Admission::Owner(entry) => entry,
    };

    // Cache the outcome so concurrent duplicates ack it; fold transport/validation
    // errors into ok:false rather than an axum error waiters could never observe.
    let response = perform_relay(
        &state,
        &sender_id,
        &cmd,
        origin,
        agent.as_ref(),
        headers.get(header::AUTHORIZATION),
    )
    .await
    .unwrap_or_else(
        |(status, message)| codeterm_service_core::mesh::types::RelayResponse {
            ok: false,
            status: status.as_u16(),
            data: None,
            error: Some(message),
        },
    );
    entry.complete(response.clone());
    if !outcome_holds_dedupe(&response) {
        super::send_dedupe::MeshDeliveryDedupe::global().forget(&key);
    }
    Ok(Json(response))
}

/// A retry-safe, definitively-not-delivered response for a saturated dedupe
/// window: nothing was written, so `bytes_written=false` tells the sender to retry.
fn saturated_response() -> codeterm_service_core::mesh::types::RelayResponse {
    relay_failure(
        StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        codeterm_mesh::contract::delivery::MESH_DELIVERY_WINDOW_SATURATED,
        Some(false),
        "mesh delivery window saturated by in-flight sends; nothing written".into(),
    )
}

/// A logical tab send (POST `.../send`).
fn is_tab_send(method: &str, path: &str) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    method.eq_ignore_ascii_case("POST") && bare.ends_with("/send")
}

/// A logical tab send gets a fresh delivery id for at-most-once dedupe; every
/// other relayed operation opts out (None).
fn relay_send_delivery_id(method: &str, path: &str) -> Option<String> {
    is_tab_send(method, path).then(|| uuid::Uuid::new_v4().to_string())
}

/// Failure deadline for the relay's own loopback call to a tab send. The send
/// route waits on the target's own readiness for up to 90s and answers with a
/// classified delivery code; a shorter deadline here discards that answer and
/// reports a bare transport error instead. Stays under the mesh client's 120s
/// relay budget so the peer's classification, not the sender's transport, decides.
const SEND_FORWARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(110);

/// Reads answer in milliseconds; they keep the short failure deadline.
const READ_FORWARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn local_forward_deadline(method: &str, path: &str) -> std::time::Duration {
    if is_tab_send(method, path) || crate::mesh_attachment::is_attachment_ingest(method, path) {
        SEND_FORWARD_DEADLINE
    } else {
        READ_FORWARD_DEADLINE
    }
}

fn local_forward_http_client() -> Result<&'static reqwest::Client, (StatusCode, String)> {
    static CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
    });
    CLIENT
        .as_ref()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// A locally generated relay failure that carries a delivery code the sender can
/// branch on. `bytes_written` is `None` when the outcome is genuinely unknown —
/// absent never means "nothing happened".
fn relay_failure(
    status: u16,
    code: &str,
    bytes_written: Option<bool>,
    message: String,
) -> codeterm_service_core::mesh::types::RelayResponse {
    let mut error = serde_json::json!({ "code": code, "message": message });
    if let Some(written) = bytes_written {
        error["bytes_written"] = serde_json::json!(written);
    }
    let data = serde_json::json!({ "ok": false, "error": error });
    codeterm_service_core::mesh::types::RelayResponse {
        ok: false,
        status,
        error: Some(relay_error_summary(status, &data)),
        data: Some(data),
    }
}

fn forward_failure_response(
    connect_failed: bool,
    timed_out: bool,
    detail: String,
) -> codeterm_service_core::mesh::types::RelayResponse {
    if connect_failed {
        relay_failure(
            StatusCode::BAD_GATEWAY.as_u16(),
            codeterm_mesh::contract::delivery::PEER_LOCAL_API_UNREACHABLE,
            Some(false),
            format!("local relay failed: {detail}"),
        )
    } else if timed_out {
        relay_failure(
            StatusCode::GATEWAY_TIMEOUT.as_u16(),
            codeterm_mesh::contract::delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED,
            None,
            format!("local delivery route did not answer within the relay deadline: {detail}"),
        )
    } else {
        relay_failure(
            StatusCode::BAD_GATEWAY.as_u16(),
            codeterm_mesh::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED,
            None,
            format!("local relay lost the route response: {detail}"),
        )
    }
}

fn local_forward_failure(
    error: &reqwest::Error,
) -> codeterm_service_core::mesh::types::RelayResponse {
    forward_failure_response(error.is_connect(), error.is_timeout(), error.to_string())
}

fn local_reply_unconfirmed(
    status: u16,
    error: impl std::fmt::Display,
) -> codeterm_service_core::mesh::types::RelayResponse {
    relay_failure(
        if status < 400 {
            StatusCode::BAD_GATEWAY.as_u16()
        } else {
            status
        },
        codeterm_mesh::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED,
        None,
        format!("The route answered, but its response body could not be read: {error}"),
    )
}

/// Keep an outcome deduped unless it is definitively-not-delivered, whose
/// safe-retry must not be lost. A classified outcome with no `bytes_written` is
/// in doubt, so it holds — releasing it would invite a duplicate write.
fn outcome_holds_dedupe(resp: &codeterm_service_core::mesh::types::RelayResponse) -> bool {
    if resp.ok {
        return true;
    }
    let Some(error) = resp.data.as_ref().and_then(|d| d.get("error")) else {
        return false;
    };
    !matches!(
        error.get("bytes_written").and_then(|b| b.as_bool()),
        Some(false)
    )
}

async fn perform_relay(
    state: &AppState,
    sender_id: &str,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    authorization: Option<&HeaderValue>,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let mut response =
        perform_relay_unredacted(state, sender_id, cmd, origin, agent, authorization).await?;
    if let Some(data) = response.data.as_mut() {
        crate::peer_redaction::redact_for_peer(data);
    }
    Ok(response)
}

fn relay_prewrite_refusal(
    state: &AppState,
    sender_id: &str,
) -> Option<codeterm_service_core::mesh::types::RelayResponse> {
    if sender_id == "local" || state.mesh.lock().traffic_open(sender_id) {
        None
    } else {
        Some(relay_failure(
            StatusCode::FORBIDDEN.as_u16(),
            codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
            Some(false),
            "peer access is no longer authorized".into(),
        ))
    }
}

fn grant_refusal() -> codeterm_service_core::mesh::types::RelayResponse {
    relay_failure(
        StatusCode::FORBIDDEN.as_u16(),
        codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
        Some(false),
        "Peer authorization changed or is unavailable".into(),
    )
}

async fn perform_relay_unredacted(
    state: &AppState,
    sender_id: &str,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    authorization: Option<&HeaderValue>,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    // debug, not info — polling peers once put 285k of these in a single
    // rotated log generation, the largest contributor to a 250 MB log dir.
    log::debug!(
        "[mesh/relay] {} {} from {}",
        cmd.method,
        cmd.path,
        sender_id
    );

    if sender_id != "local"
        && codeterm_service_core::api_support::access_policy::is_local_only_path(&cmd.path)
    {
        return Ok(grant_refusal());
    }
    validate_relay_path(&cmd.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let mut admitted_grant = None;
    if sender_id != "local" {
        let (grant, protocol_version) = {
            let mesh = state.mesh.lock();
            let Some(grant) = crate::auth::peer_grant::PeerGrant::capture(&mesh, sender_id) else {
                return Ok(grant_refusal());
            };
            (grant, mesh.paired_peers[sender_id].relay_protocol_version)
        };
        let access = grant.access();
        if !codeterm_service_core::mesh::types::is_supported_relay_protocol(protocol_version) {
            return Ok(relay_failure(
                StatusCode::UPGRADE_REQUIRED.as_u16(),
                codeterm_mesh::contract::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
                Some(false),
                codeterm_mesh::version::peer_upgrade_message(),
            ));
        }
        let method =
            Method::from_bytes(cmd.method.to_ascii_uppercase().as_bytes()).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Unsupported method: {}", cmd.method),
                )
            })?;
        let tab_owner = |tab_id: &str| crate::auth::tab_owner_peer_id(state, tab_id);
        let authorization =
            authorize_peer_relay(sender_id, access, &method, &cmd.path, &cmd.body, tab_owner);
        if !grant.current(&state.mesh.lock()) {
            return Ok(grant_refusal());
        }
        if !origin.owner_trusted()
            && matches!(
                authorization,
                PeerRelayAuthorization::WorkspaceClose { .. }
                    | PeerRelayAuthorization::WorkspaceClosePreview { .. }
            )
        {
            return Err((
                StatusCode::FORBIDDEN,
                "An agent relay cannot use peer authority".into(),
            ));
        }
        match authorization {
            PeerRelayAuthorization::Rejected => {
                return Ok(relay_failure(
                    StatusCode::FORBIDDEN.as_u16(),
                    codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
                    Some(false),
                    format!(
                        "Peer not authorized to {} {} (access_level={:?})",
                        cmd.method, cmd.path, access
                    ),
                ));
            }
            PeerRelayAuthorization::WorkspaceClose {
                peer_id,
                workspace_id,
                force,
            } => {
                return close_workspace_from_mesh_peer(state, peer_id, workspace_id, force).await;
            }
            PeerRelayAuthorization::WorkspaceClosePreview { workspace_id, .. } => {
                let response = workspace_close_preview_from_mesh_peer(state, workspace_id).await?;
                if !grant.current(&state.mesh.lock()) {
                    return Ok(grant_refusal());
                }
                return Ok(response);
            }
            PeerRelayAuthorization::MeshRpc { peer_id } => {
                return mesh_rpc_from_mesh_peer(state, peer_id, &cmd.body, origin, agent).await;
            }
            PeerRelayAuthorization::TabClose { peer_id, tab_id } => {
                return peer_authorization::close_tab_from_mesh_peer(
                    state, peer_id, tab_id, &cmd.path, origin, agent,
                )
                .await;
            }
            PeerRelayAuthorization::Forward => {}
        }
        admitted_grant = Some(grant);
    }

    let sender_name = {
        let mesh = state.mesh.lock();
        mesh.paired_peers
            .get(sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| sender_id.to_string())
    };

    let mut body = cmd.body.clone();
    tag_relay_send_source(&mut body, &cmd.method, &cmd.path, &sender_name);
    tag_relay_spawn_source(&mut body, &cmd.method, &cmd.path, sender_id, &sender_name);
    tag_relay_notification_source(&mut body, &cmd.method, &cmd.path, &sender_name);

    log::debug!(
        "[mesh/relay] branch=loopback method={} path={} source={sender_name}",
        cmd.method,
        cmd.path
    );
    let request = if sender_id == "local" {
        origin::loopback_request(state, origin, cmd, &body, agent)?
    } else if let Some(authorization) = authorization {
        origin::loopback_request_with_auth(state, cmd, &body, authorization.clone())?
    } else {
        return Ok(grant_refusal());
    };
    if admitted_grant
        .as_ref()
        .is_some_and(|grant| !grant.current(&state.mesh.lock()))
    {
        return Ok(grant_refusal());
    }
    let client = local_forward_http_client()?;

    // Recheck immediately before forwarding. Once the downstream route may
    // have written to a PTY, its result and dedupe receipt must remain intact.
    if let Some(refusal) = relay_prewrite_refusal(state, sender_id) {
        return Ok(refusal);
    }
    let resp = match client.execute(request).await {
        Ok(resp) => resp,
        Err(error) => return Ok(local_forward_failure(&error)),
    };

    let status = resp.status().as_u16();
    let is_json = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.eq_ignore_ascii_case("application/json"));
    let observation =
        codeterm_service_core::api_support::access_policy::is_observation(&cmd.method, &cmd.path);
    let result = resp.bytes().await;
    if observation
        && admitted_grant
            .as_ref()
            .is_some_and(|grant| !grant.current(&state.mesh.lock()))
    {
        return Ok(grant_refusal());
    }
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return Ok(local_reply_unconfirmed(status, error)),
    };
    let data = if is_json {
        match serde_json::from_slice(&bytes) {
            Ok(data) => data,
            Err(error) => return Ok(local_reply_unconfirmed(status, error)),
        }
    } else {
        relay_response_data(&bytes)
    };
    if status < 400 && cmd.path.contains("/send") && cmd.method.eq_ignore_ascii_case("POST") {
        let target_tab = cmd.path.split('/').nth(3).unwrap_or("unknown").to_string();
        let preview = cmd
            .body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let writer = codeterm_service_core::service::activity::ActivityWriter::new(
            state.svc.services_store.conn(),
        );
        let _ = writer.record(codeterm_service_core::service::activity::peer_relay(
            sender_id,
            &sender_name,
            &target_tab,
            &preview,
        ));
        state.svc.ui.emit(
            "service-pushed",
            serde_json::json!({"source_id": sender_name, "target": target_tab, "kind": "mesh"}),
        );
    }

    Ok(codeterm_service_core::mesh::types::RelayResponse {
        ok: status < 400,
        status,
        error: if status >= 400 {
            Some(relay_error_summary(status, &data))
        } else {
            None
        },
        data: Some(data),
    })
}

fn relay_response_data(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or_else(|_| {
        let text = String::from_utf8_lossy(bytes);
        if text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(text.trim().to_owned())
        }
    })
}

fn relay_error_summary(status: u16, data: &serde_json::Value) -> String {
    let err = data.get("error").filter(|e| !e.is_null()).unwrap_or(data);
    let message = err
        .get("message")
        .and_then(|v| v.as_str())
        .or_else(|| err.as_str())
        .filter(|message| !message.trim().is_empty());
    let Some(code) = err.get("code").and_then(|v| v.as_str()) else {
        return match message {
            Some(message) => format!("HTTP {status}: {message}"),
            None => format!("HTTP {status}"),
        };
    };
    let message = message.unwrap_or("request refused");
    match err.get("bytes_written").and_then(|v| v.as_bool()) {
        Some(true) => format!("{code}: {message} (bytes reached the target PTY)"),
        Some(false) => format!("{code}: {message} (nothing written to the target PTY)"),
        None => format!("{code}: {message}"),
    }
}

/// Read-only plugin-UI asset paths (peer `/api/plugins/{id}/ui/...`).
fn is_relay_asset_path(rel: &str) -> bool {
    !rel.contains("..") && rel.starts_with("plugins/") && rel.contains("/ui/")
}

/// Constant-time check of the LOCAL daemon api token (the iframe / stream fetch
/// carries only this — never a peer secret). Mirrors `validate_manifest_token`.
fn local_token_valid(state: &AppState, candidate: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    match state.config.lock().api.token {
        Some(ref t) if !t.is_empty() => codeterm_service_core::service::tokens::constant_time_eq(
            t.as_bytes(),
            candidate.as_bytes(),
        ),
        _ => false,
    }
}

fn copy_content_type(from: &reqwest::Response, fallback: &'static str) -> HeaderValue {
    from.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| HeaderValue::from_str(s).ok())
        .unwrap_or_else(|| HeaderValue::from_static(fallback))
}

#[derive(serde::Deserialize)]
pub struct RelayAssetQuery {
    token: Option<String>,
}

/// `GET /api/mesh/relay-asset/{peer}/{*path}?token=<LOCAL>` — byte-proxy for a
/// peer's plugin-UI document + assets (D2 §4). The iframe can't send a Bearer
/// header, so it self-verifies the LOCAL token in the query; the peer is reached
/// via a server-minted signed mesh token (never serialized to the browser).
/// Read-only, whitelisted to `plugins/{id}/ui/...` GETs.
pub async fn mesh_relay_asset(
    AxumState(state): AxumState<AppState>,
    AxumPath((peer, rel)): AxumPath<(String, String)>,
    Query(q): Query<RelayAssetQuery>,
) -> Result<Response, (StatusCode, String)> {
    if !is_relay_asset_path(&rel) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("relay-asset limited to plugins/*/ui/*: {rel}"),
        ));
    }
    // are referenced tokenless by relative <src> and resolve through this proxy.
    if rel.ends_with(".html") && !local_token_valid(&state, q.token.as_deref().unwrap_or("")) {
        return Err((StatusCode::UNAUTHORIZED, "bad or missing token".into()));
    }

    let (client, signed) = {
        let mesh = state.mesh.lock();
        let paired = mesh
            .paired_peers
            .get(&peer)
            .ok_or_else(|| peer_not_paired_error(&peer))?;
        if !codeterm_service_core::mesh::types::is_supported_relay_protocol(
            paired.relay_protocol_version,
        ) {
            return Err((
                StatusCode::UPGRADE_REQUIRED,
                origin::protocol_refusal(&paired.instance_name, paired.relay_protocol_version).1,
            ));
        }
        let client = mesh
            .client_for(&peer)
            .ok_or_else(|| peer_unroutable_error(&paired.instance_name))?;
        let signed = mesh.sign_request().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sign failed: {e}"),
            )
        })?;
        (client, signed)
    };
    // token authenticates the read-only hop (see web_routes::validate_manifest_token).
    let path = format!("/api/{rel}?token={signed}");
    validate_relay_path(&path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let resp = client.open_relay_stream(&path).await.map_err(|e| {
        (
            StatusCode::GATEWAY_TIMEOUT,
            format!("peer-unreachable: {e}"),
        )
    })?;
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = copy_content_type(&resp, "application/octet-stream");
    let mut out = Response::new(Body::from_stream(resp.bytes_stream()));
    *out.status_mut() = status;
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    Ok(out)
}

mod path_guard;
mod peer_authorization;
mod peer_resolution;
use path_guard::validate_relay_path;
use peer_authorization::{
    authorize_peer_relay, close_workspace_from_mesh_peer, mesh_rpc_from_mesh_peer,
    workspace_close_preview_from_mesh_peer, PeerRelayAuthorization,
};
use peer_resolution::{
    peer_not_paired_error, peer_unroutable_error, resolve_exact_relay_peer, resolve_relay_peer,
    AnsweringPeer, PeerRelayResponse,
};
#[cfg(test)]
use peer_resolution::{
    PEER_NAME_AMBIGUOUS_REASON, PEER_NAME_NO_ONLINE_RECORD_REASON, PEER_NOT_PAIRED_REASON,
    PEER_UNROUTABLE_REASON,
};

#[cfg(test)]
mod peer_resolution_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod delivery_error_tests;

#[cfg(test)]
mod command_round_trip_tests;

#[cfg(test)]
mod owner_gate_tests;

#[cfg(test)]
#[path = "relay/sync_access_tests.rs"]
mod sync_access_tests;

#[cfg(test)]
#[path = "relay/connection_reuse_tests.rs"]
mod connection_reuse_tests;

#[cfg(test)]
#[path = "relay/origin_tests.rs"]
mod origin_tests;
