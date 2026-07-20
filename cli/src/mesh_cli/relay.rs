//! Relay local API calls to a paired peer via signed /api/mesh/relay POSTs.

use crate::data_dir_name;

/// A relay is a logical pane send (POST `.../send`) — the only shape that gets a
/// delivery id for at-most-once dedupe on the target. Mirrors the server's
/// `tag_relay_send_source` gate.
pub(crate) fn is_send_relay(method: &str, path: &str) -> bool {
    method.eq_ignore_ascii_case("POST") && path.split('?').next().unwrap_or(path).ends_with("/send")
}

/// The stable delivery id for a logical send: an explicit env override (so a
/// caller can pin one across retries) else a fresh v4 UUID per invocation.
pub(crate) fn delivery_id_or_generate(env_value: Option<String>) -> String {
    match env_value {
        Some(value) if !value.trim().is_empty() => value,
        _ => uuid::Uuid::new_v4().to_string(),
    }
}

fn resolve_delivery_id() -> String {
    delivery_id_or_generate(std::env::var("CODETERM_DELIVERY_ID").ok())
}

/// Build the `/api/mesh/relay` envelope, stamping a delivery id onto pane sends
/// so the target dedupes retries by (authenticated sender, delivery_id).
pub(crate) fn build_relay_envelope(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> serde_json::Value {
    let mut relay = serde_json::json!({
        "method": method,
        "path": path,
        "body": body
            .and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok())
            .unwrap_or(serde_json::json!(null)),
    });
    if is_send_relay(method, path) {
        relay["delivery_id"] = serde_json::json!(resolve_delivery_id());
    }
    relay
}

/// Resolve a peer's API URL by name or instance_id via the local /api/mesh/peers.
pub(crate) fn resolve_peer_url(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
) -> Option<String> {
    let peers_resp = http
        .get(format!("{}/api/mesh/peers", base))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())?;
    let paired = peers_resp.get("paired").and_then(|v| v.as_array())?;
    let peer = paired.iter().find(|p| {
        p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
            || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
    })?;
    peer_api_url(peer)
}

/// Send a relay envelope to a peer's /api/mesh/relay and return the parsed JSON response.
pub(crate) fn relay_call(
    http: &reqwest::blocking::Client,
    peer_url: &str,
    signed_token: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<serde_json::Value, String> {
    let relay = build_relay_envelope(method, path, body);
    let resp = http
        .post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
        format!(
            "HTTP {} {}: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
            preview.trim()
        )
    })
}

/// Shortcut relay — maps old-style actions to new generic relay format.
pub fn mesh_relay_cmd(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    action: &str,
    pane: &str,
    json_mode: bool,
) {
    let (method, path, body) = match action {
        "output" => (
            "GET",
            format!(
                "/api/panes/{}/output",
                if pane.is_empty() { "general" } else { pane }
            ),
            serde_json::json!(null),
        ),
        "panes" => ("GET", "/api/panes".to_string(), serde_json::json!(null)),
        "status" => (
            "GET",
            "/api/mesh/status".to_string(),
            serde_json::json!(null),
        ),
        _ => {
            eprintln!("Unknown action: {}", action);
            std::process::exit(1);
        }
    };
    let body_str = if body.is_null() {
        None
    } else {
        Some(body.to_string())
    };
    mesh_relay_generic(
        http,
        base,
        token,
        target,
        method,
        &path,
        body_str.as_deref(),
        json_mode,
    );
}

/// Fetch a relayed API response as a JSON value without printing — used by
/// callers that need to post-process the peer's response (e.g. `pane find`
/// runs the fuzzy ranker on the peer's pane list before printing).
pub fn mesh_relay_fetch_json(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> serde_json::Value {
    let peer_url = match resolve_peer_url(http, base, token, target) {
        Some(u) => u,
        None => {
            eprintln!("Peer '{}' not found", target);
            std::process::exit(1);
        }
    };
    let signed_token = match crate::read_mesh_signed_token() {
        Some(t) => t,
        None => {
            eprintln!(
                "Error: cannot sign mesh request — no identity at {:?}.",
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(data_dir_name())
                    .join("mesh/identity.json")
            );
            std::process::exit(1);
        }
    };
    match relay_call(http, &peer_url, &signed_token, method, path, body) {
        Ok(val) => {
            if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                if val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            }
            val.get("data").cloned().unwrap_or(serde_json::Value::Null)
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Resolve a peer (refreshing once on miss), sign, POST the relay envelope, and
/// return the unwrapped `data`. Exits the process on any hard failure (peer not
/// found, unsigned, transport error, or relay `ok:false`). Shared by the
/// printing path and callers that need to post-process the response.
pub(crate) fn mesh_relay_invoke(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> serde_json::Value {
    let mut peers_json = fetch_peers(http, base, token);
    let mut peer = find_peer(&peers_json, target).cloned();
    if peer.is_none() {
        let _ = http
            .post(format!("{}/api/mesh/refresh", base))
            .header(
                "Authorization",
                format!("Bearer {}", token.as_deref().unwrap_or("")),
            )
            .send();
        peers_json = fetch_peers(http, base, token);
        peer = find_peer(&peers_json, target).cloned();
    }
    let Some(peer) = peer else {
        eprintln!("Peer '{}' not found (after refresh)", target);
        std::process::exit(1);
    };
    let Some(peer_url) = peer_api_url(&peer) else {
        eprintln!("Peer '{}' has no non-loopback API address", target);
        std::process::exit(1);
    };

    // Mesh peers require a freshly signed Ed25519 token from mesh identity.
    // Local CODETERM_API_TOKEN is not accepted by peer verifiers.
    let signed_token = match crate::read_mesh_signed_token() {
        Some(t) => t,
        None => {
            eprintln!(
                "Error: cannot sign mesh request — no identity at {:?}.",
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(data_dir_name())
                    .join("mesh/identity.json")
            );
            std::process::exit(1);
        }
    };

    let relay = build_relay_envelope(method, path, body);

    match http
        .post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay)
        .send()
    {
        Ok(resp) => {
            let status = resp.status();
            let bytes = resp.bytes().unwrap_or_default();
            let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();

            if !status.is_success() && val.is_null() {
                let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
                eprintln!(
                    "Error: peer returned HTTP {} ({}): {}",
                    status.as_u16(),
                    peer_url,
                    preview.trim()
                );
                std::process::exit(1);
            }

            // Relay returns 200+ok:false on auth/HTTP failures; surface as errors.
            if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                if val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            }

            val.get("data").cloned().unwrap_or(serde_json::Value::Null)
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

pub(crate) fn peer_api_url(peer: &serde_json::Value) -> Option<String> {
    let port = peer
        .get("api_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(7685);
    let addresses = peer.get("addresses").and_then(|v| v.as_array())?;
    let ip = addresses
        .iter()
        .filter_map(|addr| addr.get("ip").and_then(|v| v.as_str()))
        .find(|ip| !is_loopback_ip(ip))?;
    Some(format!("http://{}:{}", format_host(ip), port))
}

fn is_loopback_ip(ip: &str) -> bool {
    ip.parse::<std::net::IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

fn format_host(ip: &str) -> String {
    if ip.contains(':') && !ip.starts_with('[') {
        format!("[{}]", ip)
    } else {
        ip.to_string()
    }
}

/// Generic relay — send any API call through mesh to a remote peer and print.
#[allow(clippy::too_many_arguments)]
pub fn mesh_relay_generic(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    json_mode: bool,
) {
    let data = mesh_relay_invoke(http, base, token, target, method, path, body);

    if json_mode {
        // Unwrap envelope so `--json` output matches local CLI shape.
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
        // /panes/<id>/output returns {"text": "..."} — print raw.
        print!("{}", text);
    } else if data.is_null() {
        eprintln!("Error: unexpected response from peer (null data)");
        std::process::exit(1);
    } else {
        crate::cmd::print::print_human(&data);
    }
}

/// Build the `POST /api/remote-workers` body that registers a freshly spawned
/// cross-mesh worker on the origin. `report_to` and `model` come from the spawn
/// REQUEST body (so the registry keys on the same target the worker reports to);
/// `pane_id`/`title` come from the spawn RESPONSE. Returns `None` when there is
/// no report target or the spawn produced no pane id (nothing to nest).
pub(crate) fn remote_worker_record_body(
    peer: &str,
    spawn_body: &serde_json::Value,
    spawn_data: &serde_json::Value,
) -> Option<serde_json::Value> {
    let report_to = spawn_body
        .get("report_to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let pane_id = spawn_data
        .get("pane_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let title = spawn_data
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(pane_id);
    let mut body = serde_json::json!({
        "orchestrator_pane": report_to,
        "peer": peer,
        "remote_pane": pane_id,
        "title": title,
    });
    if let Some(m) = spawn_body
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        body["model"] = serde_json::json!(m);
    }
    Some(body)
}

pub(crate) fn remote_worker_forget_body(peer: &str, remote_pane: &str) -> serde_json::Value {
    serde_json::json!({
        "peer": peer,
        "remote_pane": remote_pane,
    })
}

/// Relay an `agent spawn` to a peer, then register the resulting worker in the
/// origin's remote-worker registry so the sidebar ghosts it under its
/// orchestrator without waiting for the first report. Registration is
/// best-effort — a spawn that succeeds is never failed by a registry hiccup.
pub fn mesh_spawn_and_register(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    body: Option<&str>,
    json_mode: bool,
) {
    let spawn_body: serde_json::Value = body
        .and_then(|b| serde_json::from_str(b).ok())
        .unwrap_or(serde_json::json!({}));
    let data = mesh_relay_invoke(http, base, token, target, "POST", "/api/agents/spawn", body);

    // the raw `--mesh` arg, which may be an id/alias and split the ghost entry.
    let peer_name = find_peer(&fetch_peers(http, base, token), target)
        .and_then(|p| p.get("instance_name").and_then(|v| v.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| target.to_string());
    if let Some(reg) = remote_worker_record_body(&peer_name, &spawn_body, &data) {
        let _ = http
            .post(format!("{}/api/remote-workers", base))
            .header(
                "Authorization",
                format!("Bearer {}", token.as_deref().unwrap_or("")),
            )
            .json(&reg)
            .send();
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else if let Some(pid) = data.get("pane_id").and_then(|v| v.as_str()) {
        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        println!("Spawned: {} (pane {}) on {}", title, pid, target);
    } else {
        crate::cmd::print::print_human(&data);
    }
}

/// Relay a remote pane close and remove any matching origin-side remote-worker
/// ghost. The registry cleanup is best-effort: a successful remote close should
/// not fail just because the origin had no stale ghost or could not delete it.
pub fn mesh_close_pane_and_forget_remote_worker(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    pane_id: &str,
    json_mode: bool,
) {
    let path = format!("/api/panes/{pane_id}/close");
    let data = mesh_relay_invoke(http, base, token, target, "POST", &path, None);

    let peer_name = find_peer(&fetch_peers(http, base, token), target)
        .and_then(|p| p.get("instance_name").and_then(|v| v.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| target.to_string());
    let _ = http
        .delete(format!("{}/api/remote-workers", base))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .json(&remote_worker_forget_body(&peer_name, pane_id))
        .send();

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else if data.is_null() {
        eprintln!("Error: unexpected response from peer (null data)");
        std::process::exit(1);
    } else {
        crate::cmd::print::print_human(&data);
    }
}

fn fetch_peers(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
) -> serde_json::Value {
    http.get(format!("{}/api/mesh/peers", base))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .unwrap_or(serde_json::json!({}))
}

fn find_peer<'a>(peers_json: &'a serde_json::Value, target: &str) -> Option<&'a serde_json::Value> {
    peers_json
        .get("paired")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find(|p| {
                p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
                    || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
            })
        })
}
