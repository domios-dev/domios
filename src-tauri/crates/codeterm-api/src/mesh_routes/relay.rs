use crate::routes::AppState;
use axum::{
    extract::State as AxumState,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    Extension, Json,
};
use std::sync::LazyLock;

mod caller;
mod capability;
mod cloud_producer;
mod cloud_wake;
#[cfg(test)]
mod cloud_wake_tests;
mod observation_limit;
pub(crate) mod origin;
mod peer_asset;
mod peer_rail;
mod peer_stream;
mod peer_stream_backoff;
pub(crate) use cloud_producer::{
    cloud_outbox_announce, resolve_cloud_ingress_endpoint, resolve_secure_input_peer,
    send_owner_secure_delivery, send_via_cloud_ingress,
};
pub use cloud_producer::{
    CloudIngressConnector, CloudIngressSendError, CLOUD_NOTIFICATION_RELAY_REFUSED,
    CLOUD_OUTBOUND_OPERATION_UNSUPPORTED,
};
#[cfg(any(test, feature = "test-support"))]
pub use cloud_producer::{CloudIngressConnectorOverride, CloudOutboxOverride};

pub(crate) fn register_extensions(builder: &mut codeterm_service_core::service::ExtensionsBuilder) {
    builder.insert_late::<codeterm_mesh::cloud_ingress::CloudIngressHandle>();
    #[cfg(any(test, feature = "test-support"))]
    {
        builder.insert_late::<CloudIngressConnectorOverride>();
        builder.insert_late::<CloudOutboxOverride>();
    }
}
#[cfg(test)]
use peer_asset::is_relay_asset_path;
pub use peer_asset::mesh_relay_asset;
#[cfg(test)]
use peer_stream::is_streamable_relay_path;
pub use peer_stream::mesh_relay_stream;

mod sync;
pub use sync::mesh_sync;
#[cfg(test)]
use sync::{sync_input_for_access, sync_output_for_access};

pub(crate) use origin::sign as sign_caller;
pub(super) use origin::{authenticate_signed_mesh_request, SignedMeshRequest};

/// Verify relay sender: must be paired. Paired = trusted.
pub(super) fn check_mesh_relay_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let caller = origin::authenticate_signed_mesh_request(state, headers)?;
    if !caller.origin.owner_trusted() {
        return Err((
            StatusCode::FORBIDDEN,
            "An agent relay cannot use peer authority".into(),
        ));
    }
    Ok(caller.peer_id)
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
    identify_supports_feature(identify, "delayed_recurrence")
}

pub(crate) const REQUIRE_AGENT_SEND_FEATURE: &str = "mesh_send_require_agent";

pub(crate) fn identify_supports_feature(identify: &serde_json::Value, name: &str) -> bool {
    identify
        .get("features")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some(name))
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
    if let crate::auth::CallerIdentity::Tab(caller) = &identity {
        capability::authorize(&state, caller, &req.method, &req.path).await?;
        caller::bind(caller, &req.method, &req.path, &mut req.body)
            .map_err(|message| (StatusCode::FORBIDDEN, message))?;
    }
    let is_login_code = path_guard::is_runtime_login_code_relay(&req.method, &req.path, &req.body);
    path_guard::refuse_non_owner_login_code(
        matches!(identity, crate::auth::CallerIdentity::Local),
        is_login_code,
        &mut req.body,
    )?;
    if let Some(early) = cloud_wake::preflight_cloud_wake(&state, &headers, &req).await? {
        if is_login_code {
            path_guard::zeroize_login_body(&mut req.body);
        }
        return Ok(Json(early));
    }
    let inbound_only = cloud_producer::has_cloud_outbox(&state);
    let target = (req.target_instance_id.clone(), req.target_peer_id.clone());
    let (request_origin, transport, peer_id, peer_name, peer_version) =
        crate::auth::off_runtime(&state, &headers, move |state, headers| {
            cloud_producer::resolve_outbound_transport(state, headers, &target, inbound_only)
        })
        .await?;
    path_guard::refuse_non_owner_login_code(
        request_origin == crate::auth::AuthenticatedOrigin::Owner,
        is_login_code,
        &mut req.body,
    )?;
    if request_origin == crate::auth::AuthenticatedOrigin::Owner {
        state
            .svc
            .cloud_machines()
            .record_owner_interaction(&peer_id);
    }
    let actor =
        crate::auth::ActorHeaders::authenticated(&identity, request_origin, headers.clone());
    let answered_by = |response| {
        Json(PeerRelayResponse {
            response,
            peer: AnsweringPeer {
                instance_id: peer_id.clone(),
                instance_name: peer_name.clone(),
            },
        })
    };
    if let cloud_producer::OutboundRelayTransport::CloudOutbox {
        target,
        origin,
        agent,
    } = transport
    {
        let resp = cloud_producer::relay_via_cloud_outbox(
            &state,
            target,
            origin,
            agent,
            &req.method,
            &req.path,
            req.body,
        )
        .await?;
        return Ok(answered_by(resp));
    }
    let transport =
        cloud_producer::ReadyOutboundTransport::prepare(&state, &peer_id, &peer_name, transport)?;
    if is_tab_send(&req.method, &req.path) && req.body["require_agent"].as_bool() == Some(true) {
        let identify = transport.fetch_identify(&state).await;
        if !identify
            .as_ref()
            .is_some_and(|identify| identify_supports_feature(identify, REQUIRE_AGENT_SEND_FEATURE))
        {
            return Ok(answered_by(relay_failure(
                StatusCode::UPGRADE_REQUIRED.as_u16(),
                "peer_lacks_agent_only_send",
                Some(false),
                format!("Cannot verify agent-only delivery on {peer_name}. Check that it is reachable and running an updated Domios."),
            )));
        }
    }
    if is_recurring_delayed_relay(&req.method, &req.path, &req.body) {
        let identify = transport.fetch_identify(&state).await;
        if let Some(refusal) = recurrence_capability_refusal_response(
            &peer_name,
            identify.as_ref(),
            peer_version.as_deref(),
        ) {
            return Ok(answered_by(refusal));
        }
    }
    let mut relay_body = req.body;
    let is_agent_spawn = req.path.split('?').next() == Some("/api/agents/spawn")
        && req.method.eq_ignore_ascii_case("POST");
    if is_agent_spawn {
        let svc = state.svc.clone();
        let callers: [Option<String>; 4] = [
            actor
                .as_ref()
                .ok()
                .and_then(crate::tab_routes::caller_tab_header),
            relay_body
                .get("spawned_by_tab_id")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            relay_body
                .get("creator_tab_id")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            relay_body
                .get("parent_tab_id")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
        ];
        tokio::task::spawn_blocking(move || {
            let refs = [
                callers[0].as_deref(),
                callers[1].as_deref(),
                callers[2].as_deref(),
                callers[3].as_deref(),
            ];
            codeterm_service_core::service::execution_sandbox_refuse_confined_mesh_delegation(
                &svc, &refs,
            )
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| (StatusCode::FORBIDDEN, error))?;
    }
    transport
        .transfer_artifacts(
            &state,
            actor
                .as_ref()
                .map(crate::tab_routes::caller_tab_header)
                .map_err(|(_, message)| message.clone()),
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
    if is_agent_spawn {
        let enrich_state = state.clone();
        let method = req.method.clone();
        let path = req.path.clone();
        relay_body = tokio::task::spawn_blocking(move || {
            enrich_spawn_ownership(&enrich_state, &method, &path, &mut relay_body);
            relay_body
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    let mut cmd = codeterm_service_core::mesh::types::RelayCommand {
        method: req.method,
        path: req.path,
        body: relay_body,
        delivery_id,
    };
    let drain_agent = match &transport {
        cloud_producer::ReadyOutboundTransport::CloudIngress { agent, .. } => agent.clone(),
        cloud_producer::ReadyOutboundTransport::Http(_) => None,
    };
    // The sender hop's own transport failure classifies into the same
    // relay_failure() vocabulary the receiving hop uses (see relay_error_summary).
    let relay_result = transport.send_relay(&state, &cmd).await;
    if is_login_code || path_guard::is_sensitive_relay(&cmd.method, &cmd.path, &cmd.body) {
        path_guard::zeroize_login_body(&mut cmd.body);
    }
    let resp = match relay_result {
        Ok(resp) => resp,
        Err(failure) => return Ok(answered_by(failure)),
    };

    let drain_agent = match drain_agent {
        Some(agent) => Some(agent),
        None if request_origin == crate::auth::AuthenticatedOrigin::Agent => {
            crate::auth::off_runtime(&state, &headers, origin::authenticated_local_caller)
                .await
                .ok()
                .and_then(|(_, agent)| agent)
        }
        None => None,
    };
    if let Some(held) =
        cloud_wake::hold_if_drain_raced(&state, &peer_id, &cmd, &resp, request_origin, drain_agent)
    {
        return Ok(answered_by(held));
    }

    // sidebar ghosts it under the report target immediately (no report yet).
    if let Some((report_to, remote_tab, title, model)) =
        spawn_registration_from_relay(&cmd.method, &cmd.path, &cmd.body, &resp)
    {
        if let Ok(orchestrator) = crate::routes::resolve_tab_async(&state, &report_to).await {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let workers = state.svc.remote_workers.clone();
            let orchestrator_tab = orchestrator.clone();
            let peer_label = peer_name.clone();
            let _ = tokio::task::spawn_blocking(move || {
                workers.record_spawn(
                    &orchestrator_tab,
                    &peer_label,
                    &remote_tab,
                    &title,
                    model,
                    now_ms,
                );
            })
            .await;
            state.svc.emit_remote_workers_changed(&orchestrator);
        }
    }

    Ok(answered_by(resp))
}

pub async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(mut cmd): Json<codeterm_service_core::mesh::types::RelayCommand>,
) -> Result<Json<codeterm_service_core::mesh::types::RelayResponse>, (StatusCode, String)> {
    let caller =
        crate::auth::off_runtime(&state, &headers, origin::authenticate_signed_mesh_request)
            .await?;
    let sender_id = &caller.peer_id;
    let origin = caller.origin;

    let Some(delivery_id) = cmd
        .delivery_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && is_tab_delivery(&cmd.method, &cmd.path))
    else {
        let outcome =
            perform_relay(&state, &caller, &cmd, headers.get(header::AUTHORIZATION)).await;
        path_guard::zeroize_login_body(&mut cmd.body);
        return outcome.map(Json);
    };

    // At-most-once (#240): one dispatch per (authenticated sender, delivery_id);
    // duplicates replay; a saturated in-flight window is rejected, never grown.
    let agent_tab = caller
        .agent
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
    let response = perform_relay(&state, &caller, &cmd, headers.get(header::AUTHORIZATION))
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

fn is_tab_exec(method: &str, path: &str) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    method.eq_ignore_ascii_case("POST") && bare.ends_with("/exec")
}

fn is_tab_delivery(method: &str, path: &str) -> bool {
    is_tab_send(method, path) || is_tab_exec(method, path)
}

/// Logical tab deliveries (`send` and `exec`) get a fresh delivery id for
/// at-most-once dedupe; every other relayed operation opts out (None).
fn relay_send_delivery_id(method: &str, path: &str) -> Option<String> {
    is_tab_delivery(method, path).then(|| uuid::Uuid::new_v4().to_string())
}

/// Failure deadline for the relay's own loopback call to a tab delivery. The
/// route waits on the target's own readiness or exec timeout and answers with
/// a classified outcome; staying under the 120s relay budget lets the peer decide.
const SEND_FORWARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(110);

/// Reads answer in milliseconds; they keep the short failure deadline.
const READ_FORWARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

fn is_provider_cli_launch(method: &str, path: &str) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    if !method.eq_ignore_ascii_case("POST") {
        return false;
    }
    let Some(rest) = bare.strip_prefix("/api/agents/providers/") else {
        return false;
    };
    let Some(provider) = rest
        .strip_suffix("/cli")
        .or_else(|| rest.strip_suffix("/install-in-tab"))
    else {
        return false;
    };
    !provider.is_empty() && !provider.contains('/')
}

pub(crate) fn local_forward_deadline(method: &str, path: &str) -> std::time::Duration {
    if is_tab_delivery(method, path)
        || crate::mesh_attachment::is_attachment_ingest(method, path)
        || is_provider_cli_launch(method, path)
    {
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

fn relay_failure_outcome(code: &str, bytes_written: Option<bool>) -> &'static str {
    use codeterm_mesh::contract::delivery;
    if code == delivery::SUBMIT_UNCONFIRMED {
        delivery::OUTCOME_SUBMIT_UNCONFIRMED
    } else if bytes_written == Some(false) {
        delivery::OUTCOME_REFUSED
    } else {
        delivery::OUTCOME_UNKNOWN
    }
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
    let outcome = relay_failure_outcome(code, bytes_written);
    let mut error = serde_json::json!({ "code": code, "outcome": outcome, "message": message });
    if let Some(written) = bytes_written {
        error["bytes_written"] = serde_json::json!(written);
    }
    let data = serde_json::json!({ "ok": false, "outcome": outcome, "error": error });
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
    caller: &SignedMeshRequest,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    authorization: Option<&HeaderValue>,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let (mut response, already_redacted) =
        perform_relay_unredacted(state, caller, cmd, authorization).await?;
    if !already_redacted {
        if let Some(data) = response.data.as_mut() {
            crate::peer_redaction::redact_for_peer(data);
        }
    }
    Ok(response)
}

#[cfg(test)]
async fn perform_test_relay(
    state: &AppState,
    sender_id: &str,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    authorization: Option<&HeaderValue>,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let caller = SignedMeshRequest {
        peer_id: sender_id.to_owned(),
        origin,
        agent: agent.cloned(),
        grant: crate::auth::peer_grant::PeerGrant::capture(&state.mesh.lock(), sender_id),
    };
    perform_relay(state, &caller, cmd, authorization).await
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
    caller: &SignedMeshRequest,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    authorization: Option<&HeaderValue>,
) -> Result<(codeterm_service_core::mesh::types::RelayResponse, bool), (StatusCode, String)> {
    let sender_id = caller.peer_id.as_str();
    let origin = caller.origin;
    let agent = caller.agent.as_ref();
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
        return Ok((grant_refusal(), true));
    }
    validate_relay_path(&cmd.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let mut admitted_grant = None;
    if sender_id != "local" {
        let Some(grant) = caller.grant.as_ref() else {
            return Ok((grant_refusal(), true));
        };
        let protocol_version = {
            let mesh = state.mesh.lock();
            if !grant.current(&mesh) {
                return Ok((grant_refusal(), true));
            }
            mesh.paired_peers[sender_id].relay_protocol_version
        };
        let access = grant.access();
        if !codeterm_service_core::mesh::types::is_supported_relay_protocol(protocol_version) {
            return Ok((
                relay_failure(
                    StatusCode::UPGRADE_REQUIRED.as_u16(),
                    codeterm_mesh::contract::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
                    Some(false),
                    codeterm_mesh::version::peer_upgrade_message(),
                ),
                true,
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
            return Ok((grant_refusal(), true));
        }
        if let Some(handled) = peer_authorization::dispatch_peer_relay(
            state,
            authorization,
            cmd,
            access,
            origin,
            agent,
            grant,
        )
        .await?
        {
            return Ok(handled);
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

    let auth_header = if sender_id == "local" {
        let loopback_state = state.clone();
        let loopback_agent = agent.cloned();
        let token = tokio::task::spawn_blocking(move || {
            origin::sign(&loopback_state, origin, loopback_agent.as_ref())
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
        format!("Bearer {token}").parse().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid signed authorization".into(),
            )
        })?
    } else if let Some(authorization) = authorization {
        authorization.clone()
    } else {
        return Ok((grant_refusal(), true));
    };
    let forward_target = origin::build_local_forward(state, cmd, &body, auth_header, &sender_name);
    path_guard::zeroize_login_body(&mut body);
    let forward_target = forward_target?;
    if admitted_grant
        .as_ref()
        .is_some_and(|grant| !grant.current(&state.mesh.lock()))
    {
        return Ok((grant_refusal(), true));
    }

    let observation =
        codeterm_service_core::api_support::access_policy::is_observation(&cmd.method, &cmd.path);
    let _observation_permit = if sender_id != "local" && observation {
        observation_limit::acquire_peer_observation(sender_id).await
    } else {
        None
    };

    // Recheck immediately before forwarding. Once the downstream route may
    // have written to a PTY, its result and dedupe receipt must remain intact.
    if let Some(refusal) = relay_prewrite_refusal(state, sender_id) {
        return Ok((refusal, true));
    }
    let (status, is_json, result) = match origin::execute_local_forward(
        forward_target,
        local_forward_deadline(&cmd.method, &cmd.path),
    )
    .await?
    {
        origin::LocalForwardOutcome::Response {
            status,
            is_json,
            body,
        } => (status, is_json, body),
        origin::LocalForwardOutcome::Failed(failure) => return Ok((failure, true)),
    };
    if observation
        && admitted_grant
            .as_ref()
            .is_some_and(|grant| !grant.current(&state.mesh.lock()))
    {
        return Ok((grant_refusal(), true));
    }
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return Ok((local_reply_unconfirmed(status, error), true)),
    };
    let send_activity =
        (status < 400 && cmd.path.contains("/send") && cmd.method.eq_ignore_ascii_case("POST"))
            .then(|| {
                let target_tab = cmd.path.split('/').nth(3).unwrap_or("unknown").to_string();
                let preview = cmd
                    .body
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>();
                (
                    sender_id.to_owned(),
                    sender_name.clone(),
                    target_tab,
                    preview,
                )
            });
    let svc = state.svc.clone();
    let reply = tokio::task::spawn_blocking(move || {
        let mut data = if is_json {
            match serde_json::from_slice(&bytes) {
                Ok(data) => data,
                Err(error) => return (local_reply_unconfirmed(status, error), None),
            }
        } else {
            relay_response_data(&bytes)
        };
        crate::peer_redaction::redact_for_peer(&mut data);
        let emitted = send_activity.map(|(sender_id, sender_name, target_tab, preview)| {
            let writer = codeterm_service_core::service::activity::ActivityWriter::new(
                svc.services_store.conn(),
            );
            let _ = writer.record(codeterm_service_core::service::activity::peer_relay(
                &sender_id,
                &sender_name,
                &target_tab,
                &preview,
            ));
            (sender_name, target_tab)
        });
        let response = codeterm_service_core::mesh::types::RelayResponse {
            ok: status < 400,
            status,
            error: if status >= 400 {
                Some(relay_error_summary(status, &data))
            } else {
                None
            },
            data: Some(data),
        };
        (response, emitted)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if let Some((sender_name, target_tab)) = reply.1 {
        state.svc.ui.emit(
            "service-pushed",
            serde_json::json!({"source_id": sender_name, "target": target_tab, "kind": "mesh"}),
        );
    }
    Ok((reply.0, true))
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

fn copy_content_type(from: &reqwest::Response, fallback: &'static str) -> HeaderValue {
    from.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| HeaderValue::from_str(s).ok())
        .unwrap_or_else(|| HeaderValue::from_static(fallback))
}

mod path_guard;
mod peer_authorization;
mod peer_resolution;
mod remote_notification;
use path_guard::validate_relay_path;
use peer_authorization::authorize_peer_relay;
#[cfg(test)]
use peer_authorization::PeerRelayAuthorization;
use peer_resolution::{
    peer_not_paired_error, peer_unroutable_error, AnsweringPeer, PeerRelayResponse,
};
#[cfg(test)]
use peer_resolution::{
    resolve_exact_relay_peer, resolve_relay_peer, PEER_NAME_AMBIGUOUS_REASON,
    PEER_NAME_NO_ONLINE_RECORD_REASON, PEER_NOT_PAIRED_REASON, PEER_UNROUTABLE_REASON,
};
pub use remote_notification::{
    MAX_PER_PEER_NOTIFICATIONS, PEER_NOTIFICATION_RATE_LIMITED, REMOTE_NOTIFICATION_REJECTED,
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
