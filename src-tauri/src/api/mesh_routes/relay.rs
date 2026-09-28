use crate::api::access_policy::check_access;
use crate::api::routes::{check_auth, AppState};
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State as AxumState},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    Json,
};

pub async fn mesh_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(payload): Json<crate::mesh::types::MeshSyncPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    log::debug!("[mesh/sync] Incoming sync: {} peers", payload.peers.len());
    check_auth(&state, &headers).map_err(|(s, e)| {
        log::warn!("[mesh/sync] Auth FAILED: {}", e);
        (s, e.to_string())
    })?;

    // Re-verify sender token here only to recover authoritative sender_id for peer bookkeeping.
    // If parsing fails, fail soft so new token formats don't disrupt gossip.
    let sender_id = sender_id_from_headers(&state, &headers);

    let mut mesh = state.mesh.lock();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Some(voucher) = sender_id.as_deref() {
        // Log only newly-learned peers — announcing every peer on every gossip
        // exchange once put 550k redundant lines in a single machine's log.
        let known_before = mesh.known_public_keys();
        mesh.apply_gossip_snapshot(voucher, payload.generation, &payload.peers, now);
        for peer_info in &payload.peers {
            if known_before.contains_key(&peer_info.instance_id) {
                continue;
            }
            log::info!(
                "[mesh/gossip] Learned about peer {} ({})",
                peer_info.instance_name,
                peer_info.instance_id
            );
        }
    } else {
        for peer_info in &payload.peers {
            mesh.apply_gossip_peer_info(peer_info);
        }
    }
    drop(mesh);

    let mut mesh = state.mesh.lock();
    let our_payload = mesh.build_sync_payload();
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::to_value(our_payload).unwrap()))
}

/// Decode the Authorization bearer token and return the signed sender's
/// instance_id, or None if the header is absent / unsigned / unverifiable.
/// Used by mesh_sync to attribute gossip vouches; auth_middleware has
/// already gated whether the request runs at all.
fn sender_id_from_headers(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return None;
    }
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.ident.identity.public_key {
        known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
    }
    crate::mesh::crown::verify_signed_request(token, &known_keys).ok()
}

/// Verify relay sender: must be paired. Paired = trusted.
pub(super) fn check_mesh_relay_auth(
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
    let peer_id = crate::mesh::crown::verify_signed_request(token, &known_keys).map_err(|_| {
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

/// Validate that a relay path won't cause SSRF or bypass the loopback bind.
/// Rejects paths with userinfo (`@`), protocol schemes, protocol-relative
/// prefixes, or paths outside `/api/` (the only namespace the relay should
/// forward to).
fn validate_relay_path(path: &str) -> Result<(), String> {
    if !path.starts_with("/api/") {
        return Err(format!("Relay path must start with /api/: {path}"));
    }
    if path.contains('@') {
        return Err(format!(
            "Relay path must not contain '@' (SSRF via userinfo): {path}"
        ));
    }
    if path.starts_with("//") {
        return Err(format!(
            "Relay path must not start with // (protocol-relative): {path}"
        ));
    }
    // Block explicit scheme prefixes embedded in the path (e.g. /api/http://evil).
    for scheme in &["http://", "https://", "file://", "ftp://", "data:"] {
        if path.contains(scheme) {
            return Err(format!(
                "Relay path must not contain scheme '{scheme}': {path}"
            ));
        }
    }
    Ok(())
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
        if let Some(ownership) = obj.get_mut("ownership").and_then(|v| v.as_object_mut()) {
            ownership.insert("peer_id".into(), serde_json::json!(sender_id));
            ownership.insert("peer_name".into(), serde_json::json!(sender_name));
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
    if obj.get("role").and_then(|v| v.as_str()) != Some("Worker") {
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
    pub target_peer_id: String,
    #[serde(default = "default_relay_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: serde_json::Value,
}

fn default_relay_method() -> String {
    "GET".to_string()
}

/// No paired record at all for this id/name — distinct from [`PEER_UNROUTABLE_REASON`],
/// which means the record exists but `client_for` couldn't build a route to it.
pub const PEER_NOT_PAIRED_REASON: &str = "peer_not_paired";
/// The paired record exists, but `MeshManager::client_for` found no route.
/// Keep this distinct from a missing record (codeterm#368).
pub const PEER_UNROUTABLE_REASON: &str = "peer_unroutable";
/// A name matched one or more paired records, but none is online — refusing
/// to guess between them (codeterm#434). Distinct from [`PEER_NOT_PAIRED_REASON`]
/// (no record at all) and from the per-peer protocol mismatch below (exactly
/// one online-but-outdated record).
pub const PEER_NAME_NO_ONLINE_RECORD_REASON: &str = "peer_name_no_online_record";

fn peer_not_paired_error(target: &str) -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        format!("{PEER_NOT_PAIRED_REASON}: {target} is not a paired peer"),
    )
}

fn peer_unroutable_error(peer_name: &str) -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "{PEER_UNROUTABLE_REASON}: peer {peer_name} is paired but has no reachable address"
        ),
    )
}

/// Refusal for a name that matched only offline/dead records. Names EVERY
/// candidate considered (both a live-turned-offline duplicate and any stale
/// leftover) plus the id form of the same relay call, as structured JSON —
/// never bake this into prose a caller would have to pattern-match.
fn peer_name_no_online_record_error(
    name: &str,
    candidates: &[crate::mesh::paired_resolve::PairedNameCandidate],
) -> (StatusCode, String) {
    let mut candidates: Vec<_> = candidates.to_vec();
    candidates.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    let body = serde_json::json!({
        "reason": PEER_NAME_NO_ONLINE_RECORD_REASON,
        "name": name,
        "candidates": candidates.iter().map(|c| serde_json::json!({
            "instance_id": c.instance_id,
            "online": c.online,
            "relay_protocol_version": c.relay_protocol_version,
            "route_by_id": { "target_peer_id": c.instance_id },
        })).collect::<Vec<_>>(),
    });
    (
        StatusCode::SERVICE_UNAVAILABLE,
        serde_json::to_string(&body).unwrap_or_else(|_| PEER_NAME_NO_ONLINE_RECORD_REASON.into()),
    )
}

/// Resolve a paired target while preserving missing, incompatible, and
/// currently-unroutable refusal states. Name resolution over duplicate
/// records sharing an `instance_name` prefers the online, highest-protocol
/// record deterministically (codeterm#434) — see
/// [`crate::mesh::paired_resolve::resolve_paired_by_name`].
fn resolve_relay_peer(
    mesh: &crate::mesh::MeshManager,
    target: &str,
) -> Result<(crate::mesh::transport::PeerClient, String, Option<String>), (StatusCode, String)> {
    let peer = crate::mesh::paired_resolve::resolve_paired_by_name(&mesh.paired_peers, target)
        .map_err(|error| match error {
            crate::mesh::paired_resolve::ResolvePairedError::NotFound => {
                peer_not_paired_error(target)
            }
            crate::mesh::paired_resolve::ResolvePairedError::NoOnlineCandidate {
                name,
                candidates,
            } => peer_name_no_online_record_error(&name, &candidates),
        })?;
    if peer.relay_protocol_version != crate::mesh::types::RELAY_PROTOCOL_VERSION {
        return Err((
            StatusCode::UPGRADE_REQUIRED,
            format!(
                "Peer {} uses incompatible Mesh relay protocol {}",
                peer.instance_name, peer.relay_protocol_version
            ),
        ));
    }
    let name = peer.instance_name.clone();
    let version = peer.version.clone();
    let client = mesh
        .client_for(&peer.instance_id)
        .ok_or_else(|| peer_unroutable_error(&name))?;
    Ok((client, name, version))
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
        version.filter(|version| !version.trim().is_empty()).unwrap_or("unknown")
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
) -> Option<crate::mesh::types::RelayResponse> {
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
    resp: &crate::mesh::types::RelayResponse,
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
    headers: HeaderMap,
    Json(req): Json<PeerRelayCommand>,
) -> Result<Json<crate::mesh::types::RelayResponse>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let (client, peer_name, peer_version) = {
        let mesh = state.mesh.lock();
        resolve_relay_peer(&mesh, &req.target_peer_id)?
    };
    if is_recurring_delayed_relay(&req.method, &req.path, &req.body) {
        let identify = client.auth_get_json("/api/identify").await.ok();
        if let Some(refusal) = recurrence_capability_refusal_response(
            &peer_name,
            identify.as_ref(),
            peer_version.as_deref(),
        ) {
            return Ok(Json(refusal));
        }
    }
    crate::api::mesh_attachment::guard_remote_send_on_host(&req.method, &req.path, &req.body)
        .await
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;
    let delivery_id = relay_send_delivery_id(&req.method, &req.path);
    let mut relay_body = req.body;
    enrich_spawn_ownership(&state, &req.method, &req.path, &mut relay_body);
    let cmd = crate::mesh::types::RelayCommand {
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
            return Ok(Json(relay_failure(
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
        if let Ok(orchestrator) = crate::api::routes::resolve_tab(&state, &report_to) {
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

    Ok(Json(resp))
}

pub async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<crate::mesh::types::RelayCommand>,
) -> Result<Json<crate::mesh::types::RelayResponse>, (StatusCode, String)> {
    let sender_id = check_mesh_relay_auth(&state, &headers)?;

    let Some(delivery_id) = cmd
        .delivery_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return perform_relay(&state, &sender_id, &cmd).await.map(Json);
    };

    // At-most-once (#240): one dispatch per (authenticated sender, delivery_id);
    // duplicates replay; a saturated in-flight window is rejected, never grown.
    let key = super::send_dedupe::dedupe_key(&sender_id, delivery_id);
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
            return Ok(Json(entry.wait().await));
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
    let response = perform_relay(&state, &sender_id, &cmd)
        .await
        .unwrap_or_else(|(status, message)| crate::mesh::types::RelayResponse {
            ok: false,
            status: status.as_u16(),
            data: None,
            error: Some(message),
        });
    entry.complete(response.clone());
    if !outcome_holds_dedupe(&response) {
        super::send_dedupe::MeshDeliveryDedupe::global().forget(&key);
    }
    Ok(Json(response))
}

/// A retry-safe, definitively-not-delivered response for a saturated dedupe
/// window: nothing was written, so `bytes_written=false` tells the sender to retry.
fn saturated_response() -> crate::mesh::types::RelayResponse {
    relay_failure(
        StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        codeterm_types::mesh::delivery::MESH_DELIVERY_WINDOW_SATURATED,
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
    if is_tab_send(method, path) || crate::api::mesh_attachment::is_attachment_ingest(method, path)
    {
        SEND_FORWARD_DEADLINE
    } else {
        READ_FORWARD_DEADLINE
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
) -> crate::mesh::types::RelayResponse {
    let mut error = serde_json::json!({ "code": code, "message": message });
    if let Some(written) = bytes_written {
        error["bytes_written"] = serde_json::json!(written);
    }
    let data = serde_json::json!({ "ok": false, "error": error });
    crate::mesh::types::RelayResponse {
        ok: false,
        status,
        error: Some(relay_error_summary(status, &data)),
        data: Some(data),
    }
}

/// The relay's own loopback call never produced a peer answer. A deadline leaves
/// the outcome unknown (the route may still be writing); anything else — a
/// refused or broken loopback connection — wrote nothing. Pure, so a test can
/// drive both arms without fabricating a transport error.
fn forward_failure_response(timed_out: bool, detail: String) -> crate::mesh::types::RelayResponse {
    if timed_out {
        relay_failure(
            StatusCode::GATEWAY_TIMEOUT.as_u16(),
            codeterm_types::mesh::delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED,
            None,
            format!("local delivery route did not answer within the relay deadline: {detail}"),
        )
    } else {
        relay_failure(
            StatusCode::BAD_GATEWAY.as_u16(),
            codeterm_types::mesh::delivery::PEER_LOCAL_API_UNREACHABLE,
            Some(false),
            format!("local relay failed: {detail}"),
        )
    }
}

fn local_forward_failure(error: &reqwest::Error) -> crate::mesh::types::RelayResponse {
    forward_failure_response(error.is_timeout(), error.to_string())
}

/// Keep an outcome deduped unless it is definitively-not-delivered, whose
/// safe-retry must not be lost. A classified outcome with no `bytes_written` is
/// in doubt, so it holds — releasing it would invite a duplicate write.
fn outcome_holds_dedupe(resp: &crate::mesh::types::RelayResponse) -> bool {
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
    cmd: &crate::mesh::types::RelayCommand,
) -> Result<crate::mesh::types::RelayResponse, (StatusCode, String)> {
    // debug, not info — polling peers once put 285k of these in a single
    // rotated log generation, the largest contributor to a 250 MB log dir.
    log::debug!(
        "[mesh/relay] {} {} from {}",
        cmd.method,
        cmd.path,
        sender_id
    );

    validate_relay_path(&cmd.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    if sender_id != "local" {
        let (access, protocol_version) = state
            .mesh
            .lock()
            .paired_peers
            .get(sender_id)
            .map(|peer| (peer.access_level.clone(), peer.relay_protocol_version))
            .unwrap_or((crate::mesh::types::AccessLevel::ReadOnly, 0));
        if protocol_version != crate::mesh::types::RELAY_PROTOCOL_VERSION {
            return Ok(relay_failure(
                StatusCode::UPGRADE_REQUIRED.as_u16(),
                codeterm_types::mesh::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
                Some(false),
                "Incompatible Mesh relay protocol; update CodeTerm".into(),
            ));
        }
        let method = Method::from_bytes(cmd.method.as_bytes()).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("Unsupported method: {}", cmd.method),
            )
        })?;
        match authorize_peer_relay(sender_id, &access, &method, &cmd.path, &cmd.body) {
            PeerRelayAuthorization::Rejected => {
                return Ok(relay_failure(
                    StatusCode::FORBIDDEN.as_u16(),
                    codeterm_types::mesh::delivery::PEER_RELAY_NOT_AUTHORIZED,
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
            PeerRelayAuthorization::Forward => {}
        }
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

    let local_url = format!("http://127.0.0.1:{}{}", state.api_port, cmd.path);
    let signed_token = state.mesh.lock().sign_request().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Sign failed: {}", e),
        )
    })?;

    let client = reqwest::Client::builder()
        .timeout(local_forward_deadline(&cmd.method, &cmd.path))
        // off the whitelisted path — forward the redirect verbatim instead.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    let req = match cmd.method.to_uppercase().as_str() {
        "GET" => client.get(&local_url),
        "POST" => client.post(&local_url).json(&body),
        "PUT" => client.put(&local_url).json(&body),
        "PATCH" => client.patch(&local_url).json(&body),
        "DELETE" => client.delete(&local_url),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unsupported method: {}", cmd.method),
            ))
        }
    };

    let resp = match req
        .header("Authorization", format!("Bearer {}", signed_token))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(error) => return Ok(local_forward_failure(&error)),
    };

    let status = resp.status().as_u16();
    let data: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!(null));

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
        let writer = crate::service::activity::ActivityWriter::new(state.svc.services_store.conn());
        let _ = writer.record(crate::service::activity::ActivityEvent::peer_relay(
            sender_id,
            &sender_name,
            &target_tab,
            &preview,
        ));
        crate::api::events::emit_everywhere(
            &state.svc.app_handle,
            "service-pushed",
            serde_json::json!({"source_id": sender_name, "target": target_tab, "kind": "mesh"}),
        );
    }

    Ok(crate::mesh::types::RelayResponse {
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

#[derive(Debug, PartialEq, Eq)]
enum PeerRelayAuthorization<'a> {
    Rejected,
    WorkspaceClose {
        peer_id: &'a str,
        workspace_id: &'a str,
        force: bool,
    },
    Forward,
}

fn authorize_peer_relay<'a>(
    peer_id: &'a str,
    access: &crate::mesh::types::AccessLevel,
    method: &Method,
    path: &'a str,
    body: &serde_json::Value,
) -> PeerRelayAuthorization<'a> {
    if !check_access(access, method, path) {
        return PeerRelayAuthorization::Rejected;
    }
    match workspace_close_target(method, path) {
        Some(workspace_id) => PeerRelayAuthorization::WorkspaceClose {
            peer_id,
            workspace_id,
            force: relay_workspace_close_force(body),
        },
        None => PeerRelayAuthorization::Forward,
    }
}

fn relay_workspace_close_force(body: &serde_json::Value) -> bool {
    body.get("force")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn workspace_close_target<'a>(method: &Method, path: &'a str) -> Option<&'a str> {
    if *method != Method::POST {
        return None;
    }
    let workspace_id = path
        .strip_prefix("/api/workspaces/")?
        .strip_suffix("/close")?;
    (!workspace_id.is_empty() && !workspace_id.contains('/')).then_some(workspace_id)
}

async fn close_workspace_from_mesh_peer(
    state: &AppState,
    peer_id: &str,
    workspace_id: &str,
    force: bool,
) -> Result<crate::mesh::types::RelayResponse, (StatusCode, String)> {
    let service = state.svc.clone();
    let peer_id = peer_id.to_string();
    let workspace_id = workspace_id.to_string();
    let result = tokio::task::spawn_blocking(move || {
        service.close_workspace_from_authenticated_mesh_peer(&peer_id, &workspace_id, force)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Workspace close task failed: {error}"),
        )
    })?;
    match result {
        Ok(()) => Ok(crate::mesh::types::RelayResponse {
            ok: true,
            status: StatusCode::OK.as_u16(),
            data: Some(serde_json::json!({ "ok": true })),
            error: None,
        }),
        Err(error) => {
            let (status, message) = crate::api::tab_routes::map_close_error(error);
            Ok(crate::mesh::types::RelayResponse {
                ok: false,
                status: status.as_u16(),
                data: None,
                error: Some(message),
            })
        }
    }
}

/// Surface a structured delivery error (`tab_not_reachable_on_peer`,
/// `tab_busy_refused`, `submit_unconfirmed`) across the relay boundary instead
/// of a bare `HTTP <status>`. `bytes_written` is echoed so a caller can never
/// read a failure as "nothing happened" when the payload already landed.
fn relay_error_summary(status: u16, data: &serde_json::Value) -> String {
    let Some(err) = data.get("error").filter(|e| e.is_object()) else {
        return format!("HTTP {}", status);
    };
    let Some(code) = err.get("code").and_then(|v| v.as_str()) else {
        return format!("HTTP {}", status);
    };
    let message = err
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("delivery failed");
    match err.get("bytes_written").and_then(|v| v.as_bool()) {
        Some(true) => format!("{code}: {message} (bytes reached the target PTY)"),
        Some(false) => format!("{code}: {message} (nothing written to the target PTY)"),
        None => format!("{code}: {message}"),
    }
}

/// The only two cursor'd SSE streams a relay target may serve. Both carry D1
/// frames unmodified so the browser's `?since=` resume works over the relay.
fn is_streamable_relay_path(bare: &str) -> bool {
    matches!(bare, "/api/events/stream" | "/api/output/stream")
}

/// Read-only plugin-UI asset paths (peer `/api/plugins/{id}/ui/...`). Never a
/// mutating plugin route — the relay-asset proxy is read-only, ui/-scoped.
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
        Some(ref t) if !t.is_empty() => {
            crate::service::tokens::constant_time_eq(t.as_bytes(), candidate.as_bytes())
        }
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
pub struct RelayStreamQuery {
    pub peer: String,
    pub path: String,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub tab: Option<String>,
}

/// `GET /api/mesh/relay-stream?peer=<id>&path=<streamable>&since=<cursor>` —
/// browser→local (local token). Opens a signed, un-timed stream to the peer's
/// own `/api/events|output/stream?since=` and pipes bytes verbatim. Peer
/// unreachable → 504 `peer-unreachable` so the client shows a banner + backoff,
/// never a silent poll.
pub async fn mesh_relay_stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<RelayStreamQuery>,
) -> Result<Response, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let diagnostic_level = crate::api::stream_diagnostics::configured_level(
        state.config.lock().diagnostics.terminal_transport,
    );

    let bare = q.path.split('?').next().unwrap_or(&q.path);
    if !is_streamable_relay_path(bare) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Not a streamable relay path: {bare}"),
        ));
    }
    // so a raw append needs no percent-encoding.
    let mut path = q.path.clone();
    let mut stream_tab_id = None;
    if let Some(ordinary_id) = q.tab.as_deref() {
        let owner = super::remote_tabs::validate_remote_stream_owner(&q.peer, ordinary_id, bare)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        stream_tab_id = Some(owner.tab_id.clone());
        path = format!("{bare}?tab_id={}", owner.tab_id);
    } else if bare == "/api/output/stream" {
        return Err((
            StatusCode::BAD_REQUEST,
            "remote tab stream requires composite tab".into(),
        ));
    }
    if let Some(since) = q.since.as_deref().filter(|s| !s.is_empty()) {
        let sep = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{sep}since={since}");
    }
    validate_relay_path(&path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let client = {
        let mesh = state.mesh.lock();
        let peer = mesh
            .paired_peers
            .get(&q.peer)
            .ok_or_else(|| peer_not_paired_error(&q.peer))?;
        if !peer.online {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "remote tab unavailable".into(),
            ));
        }
        if peer.relay_protocol_version != crate::mesh::types::RELAY_PROTOCOL_VERSION {
            return Err((
                StatusCode::UPGRADE_REQUIRED,
                format!(
                    "Peer {} uses an incompatible Mesh relay protocol",
                    peer.instance_name
                ),
            ));
        }
        mesh.client_for(&q.peer)
            .ok_or_else(|| peer_unroutable_error(&peer.instance_name))?
    };
    let resp = client.open_relay_stream(&path).await.map_err(|e| {
        (
            StatusCode::GATEWAY_TIMEOUT,
            format!("peer-unreachable: {e}"),
        )
    })?;
    if !resp.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("peer-unreachable: HTTP {}", resp.status().as_u16()),
        ));
    }

    let content_type = copy_content_type(&resp, "text/event-stream");
    let body = if bare == "/api/output/stream" {
        super::relay_stream::output_body(
            resp.bytes_stream(),
            diagnostic_level,
            q.tab.clone().unwrap_or_else(|| "*".into()),
            stream_tab_id,
            q.since.as_deref(),
        )
    } else {
        Body::from_stream(resp.bytes_stream())
    };
    let mut out = Response::new(body);
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    out.headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(out)
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
        if paired.relay_protocol_version != crate::mesh::types::RELAY_PROTOCOL_VERSION {
            return Err((
                StatusCode::UPGRADE_REQUIRED,
                format!(
                    "Peer {} uses an incompatible Mesh relay protocol",
                    paired.instance_name
                ),
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

#[cfg(test)]
mod tests;
