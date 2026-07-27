use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State as AxumState},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    Json,
};

use crate::api::routes::{check_auth, AppState};

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
        mesh.apply_gossip_snapshot(voucher, payload.generation, &payload.peers, now);
        for peer_info in &payload.peers {
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

/// Sensitive relay paths that mutate the peer (install update, change channel).
/// Read-only paths (GET checks, GET channel) are not gated and remain available
/// to any paired peer.
fn path_requires_full_access(method: &str, path: &str) -> bool {
    let m = method.to_ascii_uppercase();
    let bare = path.split('?').next().unwrap_or(path);
    if m == "DELETE" && bare.starts_with("/api/mesh/peers/") {
        return true;
    }
    matches!(
        (m.as_str(), bare),
        ("POST", "/api/update/install")
            | ("POST", "/api/update/channel")
            | ("POST", "/api/mesh/refresh")
            | ("POST", "/api/mesh/pair/request")
            | ("POST", "/api/mesh/pair/accept")
            | ("POST", "/api/mesh/introduce")
            | ("POST", "/api/mesh/rpc")
            | ("POST", "/api/agents/spawn")
            | ("POST", "/api/hooks/permission")
            | ("POST", "/api/hooks/stop")
            | ("POST", "/api/hooks/session_start")
            | ("POST", "/api/config")
            | ("PUT", "/api/config")
    )
}

/// Verify relay sender: must be paired. Paired = trusted.
fn check_mesh_relay_auth(
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

/// Stamp the authenticated originating-peer name onto a relayed `agent spawn` so
/// the spawned worker gets a durable cross-mesh report target back to the caller.
/// Mirrors [`tag_relay_send_source`]: the peer name comes from the signed token,
/// never the (spoofable) payload, and overrides any body-supplied value.
fn tag_relay_spawn_source(
    body: &mut serde_json::Value,
    method: &str,
    path: &str,
    sender_name: &str,
) {
    let bare = path.split('?').next().unwrap_or(path);
    if !(bare == "/api/agents/spawn" && method.eq_ignore_ascii_case("POST")) {
        return;
    }
    if let Some(obj) = body.as_object_mut() {
        obj.insert("origin_peer".into(), serde_json::json!(sender_name));
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
    let (client, peer_name) = {
        let mesh = state.mesh.lock();
        let client = mesh.client_for(&req.target_peer_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Peer {} not paired", req.target_peer_id),
            )
        })?;
        let name = mesh
            .paired_peers
            .get(&req.target_peer_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| req.target_peer_id.clone());
        (client, name)
    };
    let delivery_id = relay_send_delivery_id(&req.method, &req.path);
    let cmd = crate::mesh::types::RelayCommand {
        method: req.method,
        path: req.path,
        body: req.body,
        delivery_id,
    };
    let resp = client
        .send_relay(&cmd)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

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
    let data = serde_json::json!({
        "ok": false,
        "error": {
            "code": "mesh_delivery_window_saturated",
            "bytes_written": false,
            "message": "mesh delivery window saturated by in-flight sends; nothing written"
        }
    });
    crate::mesh::types::RelayResponse {
        ok: false,
        status: 503,
        error: Some(relay_error_summary(503, &data)),
        data: Some(data),
    }
}

/// A logical tab send (POST `.../send`) gets a fresh delivery id for at-most-once
/// dedupe; every other relayed operation opts out (None).
fn relay_send_delivery_id(method: &str, path: &str) -> Option<String> {
    let bare = path.split('?').next().unwrap_or(path);
    (method.eq_ignore_ascii_case("POST") && bare.ends_with("/send"))
        .then(|| uuid::Uuid::new_v4().to_string())
}

/// Keep an outcome deduped only if it delivered or is in-doubt (bytes reached the
/// PTY). Definitively-not-delivered is released so its safe-retry isn't lost.
fn outcome_holds_dedupe(resp: &crate::mesh::types::RelayResponse) -> bool {
    if resp.ok {
        return true;
    }
    matches!(
        resp.data
            .as_ref()
            .and_then(|d| d.get("error"))
            .and_then(|e| e.get("bytes_written"))
            .and_then(|b| b.as_bool()),
        Some(true)
    )
}

async fn perform_relay(
    state: &AppState,
    sender_id: &str,
    cmd: &crate::mesh::types::RelayCommand,
) -> Result<crate::mesh::types::RelayResponse, (StatusCode, String)> {
    log::info!(
        "[mesh/relay] {} {} from {}",
        cmd.method,
        cmd.path,
        sender_id
    );

    validate_relay_path(&cmd.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    if path_requires_full_access(&cmd.method, &cmd.path) && sender_id != "local" {
        let access = state
            .mesh
            .lock()
            .paired_peers
            .get(sender_id)
            .map(|p| p.access_level.clone())
            .unwrap_or(crate::mesh::types::AccessLevel::Full);
        if access != crate::mesh::types::AccessLevel::Full {
            return Ok(crate::mesh::types::RelayResponse {
                ok: false,
                status: 403,
                data: None,
                error: Some(format!(
                    "Peer not authorized to {} {} (access_level={:?})",
                    cmd.method, cmd.path, access
                )),
            });
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
    tag_relay_spawn_source(&mut body, &cmd.method, &cmd.path, &sender_name);

    let local_url = format!("http://127.0.0.1:{}{}", state.api_port, cmd.path);
    let signed_token = state.mesh.lock().sign_request().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Sign failed: {}", e),
        )
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
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

    let resp = req
        .header("Authorization", format!("Bearer {}", signed_token))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Local relay failed: {}", e),
            )
        })?;

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

    let bare = q.path.split('?').next().unwrap_or(&q.path);
    if !is_streamable_relay_path(bare) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Not a streamable relay path: {bare}"),
        ));
    }
    // so a raw append needs no percent-encoding.
    let mut path = q.path.clone();
    if let Some(since) = q.since.as_deref().filter(|s| !s.is_empty()) {
        let sep = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{sep}since={since}");
    }
    validate_relay_path(&path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let client = {
        let mesh = state.mesh.lock();
        mesh.client_for(&q.peer)
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Peer {} not paired", q.peer)))?
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
    let mut out = Response::new(Body::from_stream(resp.bytes_stream()));
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
        let client = mesh
            .client_for(&peer)
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Peer {peer} not paired")))?;
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
mod tests {
    use super::{
        is_relay_asset_path, is_streamable_relay_path, path_requires_full_access,
        tag_relay_send_source, tag_relay_spawn_source, validate_relay_path,
    };

    use super::outcome_holds_dedupe;
    use super::relay_error_summary;
    use super::relay_send_delivery_id;
    use super::spawn_registration_from_relay;
    use crate::mesh::types::RelayResponse;

    #[test]
    fn peer_relay_stamps_delivery_id_only_on_sends() {
        let id = relay_send_delivery_id("POST", "/api/tabs/abc/send")
            .expect("a forwarded Tab send carries a delivery id");
        assert!(uuid::Uuid::parse_str(&id).is_ok(), "a v4 uuid: {id}");
        assert!(
            relay_send_delivery_id("POST", "/api/tabs/abc/send?x=1").is_some(),
            "query string does not hide a send"
        );
        assert!(
            relay_send_delivery_id("GET", "/api/tabs/abc/send").is_none(),
            "a GET is never a logical send"
        );
        assert!(relay_send_delivery_id("POST", "/api/tabs").is_none());
        assert!(relay_send_delivery_id("POST", "/api/agents/spawn").is_none());
    }

    #[test]
    fn dedupe_holds_success_and_in_doubt_but_releases_undelivered() {
        let ok = RelayResponse {
            ok: true,
            status: 200,
            data: None,
            error: None,
        };
        assert!(outcome_holds_dedupe(&ok), "an optimistic success dedupes");

        let in_doubt = RelayResponse {
            ok: false,
            status: 502,
            data: Some(serde_json::json!({"error": {"bytes_written": true}})),
            error: None,
        };
        assert!(
            outcome_holds_dedupe(&in_doubt),
            "bytes reached the PTY — retrying risks a duplicate, so keep it deduped"
        );

        let not_delivered = RelayResponse {
            ok: false,
            status: 404,
            data: Some(serde_json::json!({"error": {"bytes_written": false}})),
            error: None,
        };
        assert!(
            !outcome_holds_dedupe(&not_delivered),
            "nothing was written — safe to retry, so the window must release it"
        );

        let transport = RelayResponse {
            ok: false,
            status: 502,
            data: None,
            error: Some("Local relay failed: connection refused".into()),
        };
        assert!(
            !outcome_holds_dedupe(&transport),
            "an unknown transport failure delivered nothing — allow the retry"
        );
    }

    #[test]
    fn relay_forwards_structured_delivery_code_not_bare_status() {
        let body = crate::service::delivery_error::DeliveryError::SubmitUnconfirmed {
            tab: "53854ac8".into(),
            detail: "submit not confirmed after 3s".into(),
        }
        .to_json();
        let summary = relay_error_summary(502, &body);
        assert!(summary.contains("submit_unconfirmed"), "got: {summary}");
        assert!(
            summary.contains("bytes reached the target PTY"),
            "caller must learn the payload landed; got: {summary}"
        );
        assert!(!summary.starts_with("HTTP "));
    }

    #[test]
    fn relay_marks_zero_byte_refusals_explicitly() {
        let body = crate::service::delivery_error::DeliveryError::TabBusyRefused {
            tab: "53854ac8".into(),
            detail: "still booting".into(),
        }
        .to_json();
        let summary = relay_error_summary(409, &body);
        assert!(summary.contains("tab_busy_refused"), "got: {summary}");
        assert!(summary.contains("nothing written"), "got: {summary}");
    }

    #[test]
    fn relay_unreachable_tab_is_named_not_bare_404() {
        let body = crate::service::delivery_error::DeliveryError::TabNotReachableOnPeer {
            tab: "ghost".into(),
        }
        .to_json();
        let summary = relay_error_summary(404, &body);
        assert!(
            summary.contains("tab_not_reachable_on_peer"),
            "got: {summary}"
        );
        assert!(summary.contains("nothing written"), "got: {summary}");
    }

    #[test]
    fn relay_falls_back_to_status_for_unstructured_bodies() {
        assert_eq!(
            relay_error_summary(500, &serde_json::json!(null)),
            "HTTP 500"
        );
        assert_eq!(
            relay_error_summary(403, &serde_json::json!({"error": "plain string"})),
            "HTTP 403"
        );
    }

    fn ok_resp(data: serde_json::Value) -> RelayResponse {
        RelayResponse {
            ok: true,
            status: 200,
            data: Some(data),
            error: None,
        }
    }

    #[test]
    fn spawn_relay_registration_extracts_worker_fields() {
        let body = serde_json::json!({"agent":"claude","report_to":"4d479b6c","model":"opus"});
        let resp = ok_resp(serde_json::json!({"tab_id":"53854ac8","title":"Canvas"}));
        let (report_to, remote_tab, title, model) =
            spawn_registration_from_relay("POST", "/api/agents/spawn", &body, &resp)
                .expect("spawn relay registers");
        assert_eq!(report_to, "4d479b6c");
        assert_eq!(remote_tab, "53854ac8");
        assert_eq!(title, "Canvas");
        assert_eq!(model.as_deref(), Some("opus"));
    }

    #[test]
    fn spawn_relay_registration_skips_non_spawn_and_unregisterable() {
        let body = serde_json::json!({"report_to":"main"});
        let resp = ok_resp(serde_json::json!({"tab_id":"p1"}));
        // Wrong path / method.
        assert!(spawn_registration_from_relay("POST", "/api/tabs/x/send", &body, &resp).is_none());
        assert!(spawn_registration_from_relay("GET", "/api/agents/spawn", &body, &resp).is_none());
        // No report target.
        let no_report = serde_json::json!({"agent":"claude"});
        assert!(
            spawn_registration_from_relay("POST", "/api/agents/spawn", &no_report, &resp).is_none()
        );
        // Failed relay (ok:false) must not register.
        let failed = RelayResponse {
            ok: false,
            status: 500,
            data: Some(serde_json::json!({"tab_id":"p1"})),
            error: Some("boom".into()),
        };
        assert!(
            spawn_registration_from_relay("POST", "/api/agents/spawn", &body, &failed).is_none()
        );
    }

    #[test]
    fn relay_spawn_tags_origin_peer() {
        let mut body = serde_json::json!({"agent": "claude", "report_to": "main"});
        tag_relay_spawn_source(&mut body, "POST", "/api/agents/spawn", "Mac-Air");
        assert_eq!(body["origin_peer"], serde_json::json!("Mac-Air"));
        assert_eq!(body["report_to"], serde_json::json!("main"));
    }

    #[test]
    fn relay_spawn_origin_overrides_spoofed_body() {
        let mut body = serde_json::json!({"agent": "claude", "origin_peer": "I-AM-ROOT"});
        tag_relay_spawn_source(&mut body, "post", "/api/agents/spawn", "Mac-Air");
        assert_eq!(body["origin_peer"], serde_json::json!("Mac-Air"));
    }

    #[test]
    fn relay_spawn_tagger_ignores_non_spawn_paths() {
        let mut body = serde_json::json!({"text": "hi"});
        tag_relay_spawn_source(&mut body, "POST", "/api/tabs/x/send", "Mac-Air");
        assert_eq!(body.get("origin_peer"), None);
        let mut body2 = serde_json::json!({});
        tag_relay_spawn_source(&mut body2, "GET", "/api/agents/spawn", "Mac-Air");
        assert_eq!(
            body2.get("origin_peer"),
            None,
            "GET spawn must not be tagged"
        );
    }

    #[test]
    fn relay_send_tags_originating_peer() {
        let mut body = serde_json::json!({"text": "hi", "source_tab": "e8a0ac20"});
        tag_relay_send_source(&mut body, "POST", "/api/tabs/general/send", "BLACKSTAR");
        assert_eq!(body["source_peer"], serde_json::json!("BLACKSTAR"));
        // Text is NOT pre-wrapped — the send handler owns prefixing.
        assert_eq!(body["text"], serde_json::json!("hi"));
        assert_eq!(body["source_tab"], serde_json::json!("e8a0ac20"));
    }

    #[test]
    fn relay_send_peer_overrides_spoofed_body_value() {
        let mut body = serde_json::json!({"text": "hi", "source_peer": "I-AM-ROOT"});
        tag_relay_send_source(&mut body, "post", "/api/tabs/x/send", "BLACKSTAR");
        assert_eq!(body["source_peer"], serde_json::json!("BLACKSTAR"));
    }

    #[test]
    fn relay_non_send_paths_untouched() {
        let mut body = serde_json::json!({"foo": "bar"});
        tag_relay_send_source(&mut body, "GET", "/api/tabs", "BLACKSTAR");
        assert_eq!(body.get("source_peer"), None);
        let mut body2 = serde_json::json!({"foo": "bar"});
        tag_relay_send_source(&mut body2, "GET", "/api/tabs/x/send", "BLACKSTAR");
        assert_eq!(
            body2.get("source_peer"),
            None,
            "GET /send must not be tagged"
        );
    }

    #[test]
    fn valid_relay_paths_pass() {
        assert!(validate_relay_path("/api/tabs/abc/output").is_ok());
        assert!(validate_relay_path("/api/tabs/abc/send").is_ok());
        assert!(validate_relay_path("/api/mesh/status").is_ok());
        assert!(validate_relay_path("/api/workspaces").is_ok());
    }

    #[test]
    fn non_api_paths_rejected() {
        assert!(validate_relay_path("/etc/passwd").is_err());
        assert!(validate_relay_path("/api").is_err());
        assert!(validate_relay_path("/api/").is_ok()); // bare minimum
    }

    #[test]
    fn userinfo_ssrf_rejected() {
        assert!(validate_relay_path("/api/tabs/send?url=http://evil@evil.com").is_err());
    }

    #[test]
    fn scheme_prefix_rejected() {
        assert!(validate_relay_path("/api/http://evil.com").is_err());
        assert!(validate_relay_path("/api/https://evil.com").is_err());
        assert!(validate_relay_path("/api/file:///etc/passwd").is_err());
    }

    #[test]
    fn protocol_relative_rejected() {
        assert!(validate_relay_path("//evil.com/api/tabs").is_err());
    }

    #[test]
    fn install_update_is_gated() {
        assert!(path_requires_full_access("POST", "/api/update/install"));
        assert!(path_requires_full_access(
            "POST",
            "/api/update/install?method=auto"
        ));
        assert!(path_requires_full_access(
            "post",
            "/api/update/install?method=auto&manifest_url=foo"
        ));
    }

    #[test]
    fn set_channel_is_gated() {
        assert!(path_requires_full_access("POST", "/api/update/channel"));
    }

    #[test]
    fn check_and_get_channel_are_open() {
        assert!(!path_requires_full_access("GET", "/api/update/check"));
        assert!(!path_requires_full_access(
            "GET",
            "/api/update/check?force=true"
        ));
        assert!(!path_requires_full_access("GET", "/api/update/channel"));
    }

    #[test]
    fn unrelated_paths_are_open() {
        assert!(!path_requires_full_access(
            "POST",
            "/api/tabs/general/send"
        ));
        assert!(!path_requires_full_access("GET", "/api/mesh/peers"));
    }

    #[test]
    fn mesh_refresh_is_gated() {
        assert!(path_requires_full_access("POST", "/api/mesh/refresh"));
        assert!(!path_requires_full_access("GET", "/api/mesh/refresh"));
    }

    #[test]
    fn mesh_announce_is_open() {
        // Access gate already validates signed token; re-verify sender_id during apply for relay safety.
        assert!(!path_requires_full_access("POST", "/api/mesh/announce"));
    }

    #[test]
    fn mesh_pairing_endpoints_are_gated() {
        assert!(path_requires_full_access("POST", "/api/mesh/pair/request"));
        assert!(path_requires_full_access("POST", "/api/mesh/pair/accept"));
        assert!(path_requires_full_access("POST", "/api/mesh/introduce"));
    }

    #[test]
    fn mesh_rpc_requires_full_access() {
        assert!(path_requires_full_access("POST", "/api/mesh/rpc"));
    }

    #[test]
    fn agent_spawn_is_gated() {
        assert!(path_requires_full_access("POST", "/api/agents/spawn"));
    }

    #[test]
    fn hook_endpoints_are_gated() {
        assert!(path_requires_full_access("POST", "/api/hooks/permission"));
        assert!(path_requires_full_access("POST", "/api/hooks/stop"));
        assert!(path_requires_full_access(
            "POST",
            "/api/hooks/session_start"
        ));
    }

    #[test]
    fn config_mutation_is_gated() {
        assert!(path_requires_full_access("POST", "/api/config"));
        assert!(path_requires_full_access("PUT", "/api/config"));
        assert!(!path_requires_full_access("GET", "/api/config"));
    }

    #[test]
    fn only_cursor_streams_are_relayable() {
        assert!(is_streamable_relay_path("/api/events/stream"));
        assert!(is_streamable_relay_path("/api/output/stream"));
        assert!(!is_streamable_relay_path("/api/tabs/abc/send"));
        assert!(!is_streamable_relay_path("/api/config"));
    }

    #[test]
    fn relay_asset_whitelist_is_ui_scoped() {
        assert!(is_relay_asset_path("plugins/git/ui/index.html"));
        assert!(is_relay_asset_path("plugins/git/ui/assets/app.js"));
        // Not a ui/ path, not a plugin path, or traversal — all rejected.
        assert!(!is_relay_asset_path("plugins/git/manifest.json"));
        assert!(!is_relay_asset_path("tabs/abc/ui/x"));
        assert!(!is_relay_asset_path("plugins/git/ui/../../../etc/passwd"));
    }

    #[test]
    fn peer_delete_is_gated() {
        assert!(path_requires_full_access(
            "DELETE",
            "/api/mesh/peers/some-uuid"
        ));
        assert!(path_requires_full_access(
            "DELETE",
            "/api/mesh/peers/peer-abc-123"
        ));
        assert!(!path_requires_full_access(
            "GET",
            "/api/mesh/peers/some-uuid"
        ));
    }
}
