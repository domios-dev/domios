//! Relay local API calls to a paired peer via signed /api/mesh/relay POSTs.

#[cfg(test)]
use super::compat::{
    dialect_from_identify, request_for_dialect, response_for_dialect, MeshApiDialect,
};

/// A logical tab send (POST `.../send`) — the only relay shape that gets a
/// delivery id for at-most-once dedupe on the target.
#[cfg(test)]
pub(crate) fn is_send_relay(method: &str, path: &str) -> bool {
    method.eq_ignore_ascii_case("POST") && path.split('?').next().unwrap_or(path).ends_with("/send")
}

/// The stable delivery id for a logical send: an explicit env override (so a
/// caller can pin one across retries) else a fresh v4 UUID per invocation.
#[cfg(test)]
pub(crate) fn delivery_id_or_generate(env_value: Option<String>) -> String {
    match env_value {
        Some(value) if !value.trim().is_empty() => value,
        _ => uuid::Uuid::new_v4().to_string(),
    }
}

#[cfg(test)]
fn resolve_delivery_id() -> String {
    delivery_id_or_generate(std::env::var("CODETERM_DELIVERY_ID").ok())
}

/// Build the `/api/mesh/relay` envelope, stamping a delivery id onto tab sends
/// so the target dedupes retries by (authenticated sender, delivery_id).
#[cfg(test)]
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

/// Shortcut relay — maps old-style actions to new generic relay format.
pub fn mesh_relay_cmd(target: &str, action: &str, tab: &str, json_mode: bool) {
    let (method, path, body) = match action {
        "output" => (
            "GET",
            format!(
                "/api/tabs/{}/output",
                if tab.is_empty() { "general" } else { tab }
            ),
            serde_json::json!(null),
        ),
        "tabs" => ("GET", "/api/tabs".to_string(), serde_json::json!(null)),
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
    mesh_relay_generic(target, method, &path, body_str.as_deref(), json_mode);
}

/// Fetch a relayed API response as a JSON value without printing — used by
/// callers that need to post-process the peer's response (e.g. `Tab find`
/// runs the fuzzy ranker on the peer's tab list before printing).
pub fn mesh_relay_fetch_json(
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> serde_json::Value {
    match mesh_relay_invoke_result(target, method, path, body) {
        Ok(val) => val,
        Err(failure) => failure.print_and_exit(false),
    }
}

/// A relay/spawn failure: peer, reason, and a retry hint, for both human and
/// script output.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshSpawnFailure {
    pub(crate) peer: String,
    pub(crate) reason: String,
    pub(crate) retry_hint: String,
}

impl MeshSpawnFailure {
    fn new(peer: &str, reason: impl Into<String>, retry_hint: impl Into<String>) -> Self {
        Self {
            peer: peer.to_string(),
            reason: reason.into(),
            retry_hint: retry_hint.into(),
        }
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ok": false,
            "error": {
                "peer": self.peer,
                "reason": self.reason,
                "retry_hint": self.retry_hint,
            },
        })
    }

    /// Render and exit(1); kept apart from classification so that stays
    /// testable without ending the test process.
    pub(crate) fn print_and_exit(&self, json_mode: bool) -> ! {
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&self.to_json()).unwrap());
        } else {
            eprintln!(
                "Error: mesh request to '{}' failed: {}\nhint: {}",
                self.peer, self.reason, self.retry_hint
            );
        }
        std::process::exit(1);
    }
}

impl std::fmt::Display for MeshSpawnFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.peer, self.reason)
    }
}

/// Fallback next-step hint by HTTP status class, used when the peer's
/// response carries no `retry_hint` of its own.
fn generic_retry_hint(status: u16) -> String {
    match status {
        429 => "rate limited — back off and retry shortly".to_string(),
        500..=599 => {
            "transient — retry once; if it repeats, check the peer daemon is up (`codeterm mesh peers`)"
                .to_string()
        }
        400..=499 => "request rejected — fix the spawn arguments before retrying".to_string(),
        _ => "retry the spawn".to_string(),
    }
}

/// Classify a completed relay response into success data or a structured
/// failure. Pure — no I/O, no process exit — so tests can drive it directly.
fn classify_relay_response(
    peer_name: &str,
    status: reqwest::StatusCode,
    bytes: &[u8],
) -> Result<serde_json::Value, MeshSpawnFailure> {
    let val: serde_json::Value = serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null);

    // A bare-status, non-JSON body (e.g. an unstructured axum `(StatusCode,
    // String)` error) — carry the raw text so the reason is never empty.
    if !status.is_success() && val.is_null() {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
        let reason = if preview.trim().is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {}", status.as_u16(), preview.trim())
        };
        return Err(MeshSpawnFailure::new(
            peer_name,
            reason,
            generic_retry_hint(status.as_u16()),
        ));
    }

    // Relay returns 200+ok:false on auth/HTTP failures inside the envelope.
    if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
        if val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
            let hint = val
                .get("data")
                .and_then(|d| d.get("error"))
                .and_then(|e| e.get("retry_hint"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| generic_retry_hint(status.as_u16()));
            return Err(MeshSpawnFailure::new(peer_name, err.to_string(), hint));
        }
    }

    Ok(val.get("data").cloned().unwrap_or(serde_json::Value::Null))
}

/// POST an already-built relay envelope to a resolved peer URL and classify
/// the outcome. Bypasses peer discovery, so a test double can stand in.
#[cfg(test)]
pub(crate) fn perform_mesh_relay_call(
    http: &reqwest::blocking::Client,
    peer_url: &str,
    signed_token: &str,
    peer_name: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<serde_json::Value, MeshSpawnFailure> {
    let relay = build_relay_envelope(method, path, body);
    let resp = http
        .post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay)
        .send()
        .map_err(|e| {
            MeshSpawnFailure::new(
                peer_name,
                format!("transport error: {e}"),
                "the peer may be down or unreachable — retry once, then check `codeterm mesh peers`",
            )
        })?;
    let status = resp.status();
    let bytes = resp.bytes().unwrap_or_default();
    classify_relay_response(peer_name, status, &bytes)
}

#[cfg(test)]
fn perform_compatible_mesh_relay_call(
    http: &reqwest::blocking::Client,
    peer_url: &str,
    signed_token: &str,
    peer_name: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<serde_json::Value, MeshSpawnFailure> {
    let dialect = if path == "/api/identify" {
        MeshApiDialect::Current
    } else {
        let identify = perform_mesh_relay_call(
            http,
            peer_url,
            signed_token,
            peer_name,
            "GET",
            "/api/identify",
            None,
        )?;
        dialect_from_identify(&identify)
    };
    let (path, body) = request_for_dialect(dialect, path, body);
    perform_mesh_relay_call(
        http,
        peer_url,
        signed_token,
        peer_name,
        method,
        &path,
        body.as_deref(),
    )
    .map(|value| response_for_dialect(dialect, value))
}

/// Resolve a peer (refreshing once on miss), sign, and relay — returning a
/// structured failure instead of exiting so callers can decide what to do.
pub(crate) fn mesh_relay_invoke_result(
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<serde_json::Value, MeshSpawnFailure> {
    let local = crate::build_local_api().map_err(|error| {
        MeshSpawnFailure::new(
            target,
            format!("local socket unavailable: {error}"),
            "start CodeTerm locally, then retry",
        )
    })?;
    let relay_body = body
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
        .unwrap_or(serde_json::Value::Null);
    let response = local
        .post("/api/mesh/relay-to")
        .json(&serde_json::json!({
            "target_peer_id": target,
            "method": method,
            "path": path,
            "body": relay_body,
        }))
        .send()
        .map_err(|error| {
            MeshSpawnFailure::new(
                target,
                format!("local relay failed: {error}"),
                "check `codeterm mesh peers`, then retry",
            )
        })?;
    let status = response.status();
    let bytes = response.bytes().unwrap_or_default();
    classify_relay_response(target, status, &bytes)
}

/// Like `mesh_relay_invoke_result`, but prints and exits(1) on failure —
/// the shared entry point for callers that don't need the Result themselves.
pub(crate) fn mesh_relay_invoke(
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    json_mode: bool,
) -> serde_json::Value {
    match mesh_relay_invoke_result(target, method, path, body) {
        Ok(data) => data,
        Err(failure) => failure.print_and_exit(json_mode),
    }
}

#[cfg(test)]
pub(crate) fn peer_api_url(peer: &serde_json::Value) -> Option<String> {
    if let Some(url) = peer_tailscale_https_url(peer) {
        return Some(url);
    }
    let port = peer
        .get("api_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(7685);
    let addresses = peer.get("addresses").and_then(|v| v.as_array())?;
    let ip = addresses
        .iter()
        .find(|address| {
            address
                .get("is_tailscale")
                .and_then(|value| value.as_bool())
                == Some(true)
                && address
                    .get("ip")
                    .and_then(|value| value.as_str())
                    .is_some_and(is_tailscale_ip)
        })
        .or_else(|| {
            addresses.iter().find(|address| {
                address
                    .get("ip")
                    .and_then(|value| value.as_str())
                    .is_some_and(|ip| !is_loopback_ip(ip))
            })
        })?
        .get("ip")?
        .as_str()?;
    Some(format!("http://{}:{}", format_host(ip), port))
}

#[cfg(test)]
fn peer_tailscale_https_url(peer: &serde_json::Value) -> Option<String> {
    peer.get("web_ui")?
        .get("endpoints")?
        .as_array()?
        .iter()
        .find_map(|endpoint| {
            if endpoint.get("transport").and_then(|value| value.as_str()) != Some("tailscale_https")
            {
                return None;
            }
            let host = endpoint.get("host")?.as_str()?.to_ascii_lowercase();
            let port = endpoint.get("port")?.as_u64()?;
            let valid = port > 0
                && port <= u16::MAX as u64
                && host.len() <= 253
                && host.ends_with(".ts.net")
                && host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
            valid.then(|| {
                if port == 443 {
                    format!("https://{host}")
                } else {
                    format!("https://{host}:{port}")
                }
            })
        })
}

#[cfg(test)]
fn is_loopback_ip(ip: &str) -> bool {
    ip.parse::<std::net::IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

#[cfg(test)]
fn is_tailscale_ip(ip: &str) -> bool {
    ip.parse::<std::net::Ipv4Addr>()
        .map(|address| {
            let octets = address.octets();
            octets[0] == 100 && (64..128).contains(&octets[1])
        })
        .unwrap_or(false)
}

#[cfg(test)]
fn format_host(ip: &str) -> String {
    if ip.contains(':') && !ip.starts_with('[') {
        format!("[{}]", ip)
    } else {
        ip.to_string()
    }
}

/// Generic relay — send any API call through mesh to a remote peer and print.
pub fn mesh_relay_generic(
    target: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    json_mode: bool,
) {
    let data = mesh_relay_invoke(target, method, path, body, json_mode);

    if json_mode {
        // Unwrap envelope so `--json` output matches local CLI shape.
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
        // /tabs/<id>/output returns {"text": "..."} — print raw.
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
/// `tab_id`/`title` come from the spawn RESPONSE. Returns `None` when there is
/// no report target or the spawn produced no tab id (nothing to nest).
#[cfg(test)]
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
    let tab_id = spawn_data
        .get("tab_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let title = spawn_data
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(tab_id);
    let mut body = serde_json::json!({
        "orchestrator_tab": report_to,
        "peer": peer,
        "remote_tab": tab_id,
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

pub(crate) fn remote_worker_forget_body(peer: &str, remote_tab: &str) -> serde_json::Value {
    serde_json::json!({
        "peer": peer,
        "remote_tab": remote_tab,
    })
}

/// Spawn against an already-resolved peer, registering only on success — the
/// gating logic both `mesh_spawn_and_maybe_register` and tests call directly.
#[cfg(test)]
pub(crate) fn spawn_and_maybe_register_at_resolved_peer(
    http: &reqwest::blocking::Client,
    peer_url: &str,
    signed_token: &str,
    peer_name: &str,
    origin_base: &str,
    origin_token: &Option<String>,
    spawn_body: &serde_json::Value,
    body: Option<&str>,
) -> Result<serde_json::Value, MeshSpawnFailure> {
    let data = perform_compatible_mesh_relay_call(
        http,
        peer_url,
        signed_token,
        peer_name,
        "POST",
        "/api/agents/spawn",
        body,
    )?;
    if let Some(reg) = remote_worker_record_body(peer_name, spawn_body, &data) {
        let _ = http
            .post(format!("{origin_base}/api/remote-workers"))
            .header(
                "Authorization",
                format!("Bearer {}", origin_token.as_deref().unwrap_or("")),
            )
            .json(&reg)
            .send();
    }
    Ok(data)
}

/// Resolve `target`, then spawn + conditionally register at that peer.
pub(crate) fn mesh_spawn_and_maybe_register(
    target: &str,
    spawn_body: &serde_json::Value,
    body: Option<&str>,
) -> Result<serde_json::Value, MeshSpawnFailure> {
    let _ = spawn_body;
    mesh_relay_invoke_result(target, "POST", "/api/agents/spawn", body)
}

/// Relay an `agent spawn` to a peer and print the outcome — the CLI-facing
/// wrapper around `mesh_spawn_and_maybe_register`.
pub fn mesh_spawn_and_register(target: &str, body: Option<&str>, json_mode: bool) {
    let spawn_body: serde_json::Value = body
        .and_then(|b| serde_json::from_str(b).ok())
        .unwrap_or(serde_json::json!({}));
    let data = match mesh_spawn_and_maybe_register(target, &spawn_body, body) {
        Ok(data) => data,
        Err(failure) => failure.print_and_exit(json_mode),
    };

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else if let Some(pid) = data.get("tab_id").and_then(|v| v.as_str()) {
        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        println!("Spawned: {} (Tab {}) on {}", title, pid, target);
    } else {
        crate::cmd::print::print_human(&data);
    }
}

/// Relay a remote tab close and remove any matching origin-side remote-worker
/// ghost. The registry cleanup is best-effort: a successful remote close should
/// not fail just because the origin had no stale ghost or could not delete it.
pub fn mesh_close_tab_and_forget_remote_worker(
    base: &str,
    target: &str,
    tab_id: &str,
    json_mode: bool,
) {
    let path = format!("/api/tabs/{tab_id}/close");
    let data = mesh_relay_invoke(target, "POST", &path, None, json_mode);

    if let Ok(local) = crate::build_local_api() {
        let _ = local
            .delete(format!("{}/api/remote-workers", base))
            .json(&remote_worker_forget_body(target, tab_id))
            .send();
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else if data.is_null() {
        eprintln!("Error: unexpected response from peer (null data)");
        std::process::exit(1);
    } else {
        crate::cmd::print::print_human(&data);
    }
}

#[cfg(test)]
mod mesh_spawn_failure_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn peer_api_url_prefers_verified_tailscale_https() {
        let peer = serde_json::json!({
            "api_port": 7685,
            "addresses": [{"ip": "100.84.159.37"}],
            "web_ui": {
                "protocol_version": 1,
                "endpoints": [{
                    "transport": "tailscale_https",
                    "host": "blackstar.example.ts.net",
                    "port": 443
                }]
            }
        });

        assert_eq!(
            peer_api_url(&peer).as_deref(),
            Some("https://blackstar.example.ts.net")
        );
    }

    #[test]
    fn peer_api_url_rejects_untrusted_https_host_and_uses_ip() {
        let peer = serde_json::json!({
            "api_port": 7685,
            "addresses": [{"ip": "100.84.159.37"}],
            "web_ui": {
                "protocol_version": 1,
                "endpoints": [{
                    "transport": "tailscale_https",
                    "host": "attacker.example.com",
                    "port": 443
                }]
            }
        });

        assert_eq!(
            peer_api_url(&peer).as_deref(),
            Some("http://100.84.159.37:7685")
        );
    }

    #[test]
    fn peer_api_url_prefers_tailscale_http_then_lan() {
        let peer = serde_json::json!({
            "api_port": 7685,
            "addresses": [
                {"ip": "192.168.1.195", "is_tailscale": false},
                {"ip": "100.84.159.37", "is_tailscale": true}
            ]
        });
        assert_eq!(
            peer_api_url(&peer).as_deref(),
            Some("http://100.84.159.37:7685")
        );

        let lan_only = serde_json::json!({
            "api_port": 7685,
            "addresses": [{"ip": "192.168.1.195", "is_tailscale": false}]
        });
        assert_eq!(
            peer_api_url(&lan_only).as_deref(),
            Some("http://192.168.1.195:7685")
        );
    }

    #[test]
    fn classify_bare_status_text_body_carries_a_non_empty_reason() {
        // The old daemon shape for a spawn failure: axum `(StatusCode, String)`
        // renders as plain text, not JSON — this reproduces that exact body.
        let failure = classify_relay_response(
            "blackstar",
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            b"checkout guard rejected: dirty worktree",
        )
        .expect_err("a non-2xx body must classify as a failure, never as data");
        assert_eq!(failure.peer, "blackstar");
        assert!(failure.reason.contains("500"));
        assert!(failure.reason.contains("checkout guard rejected"));
        assert!(!failure.retry_hint.is_empty());
    }

    #[test]
    fn classify_structured_error_prefers_server_supplied_retry_hint() {
        // The new daemon shape (agent_routes::SpawnRejection) round-tripped
        // through the relay envelope's `data` field.
        let body = serde_json::json!({
            "ok": false,
            "status": 500,
            "error": "spawn_failed: checkout guard rejected: dirty worktree",
            "data": {
                "ok": false,
                "error": {
                    "code": "spawn_failed",
                    "message": "checkout guard rejected: dirty worktree",
                    "retry_hint": "clean the worktree, then retry",
                }
            }
        });
        let failure = classify_relay_response(
            "blackstar",
            reqwest::StatusCode::OK,
            body.to_string().as_bytes(),
        )
        .expect_err("ok:false inside the envelope must classify as a failure");
        assert_eq!(failure.peer, "blackstar");
        assert_eq!(
            failure.reason,
            "spawn_failed: checkout guard rejected: dirty worktree"
        );
        assert_eq!(failure.retry_hint, "clean the worktree, then retry");
    }

    #[test]
    fn classify_success_returns_unwrapped_data() {
        let body = serde_json::json!({"ok": true, "status": 200, "data": {"tab_id": "abc"}});
        let data = classify_relay_response(
            "blackstar",
            reqwest::StatusCode::OK,
            body.to_string().as_bytes(),
        )
        .expect("ok:true must classify as success");
        assert_eq!(data.get("tab_id").and_then(|v| v.as_str()), Some("abc"));
    }

    /// Accept exactly one connection, then hang up without writing anything —
    /// the CLI's view of a peer daemon that dies mid-request.
    fn spawn_dying_peer() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind dying-peer listener");
        let addr = listener.local_addr().expect("listener has a local addr");
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });
        format!("http://{}", addr)
    }

    /// Stand-in ORIGIN daemon: records whether `/api/remote-workers` was hit.
    fn spawn_origin_recording_registration(registered: Arc<AtomicBool>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind origin listener");
        let addr = listener.local_addr().expect("listener has a local addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                if request.contains("/api/remote-workers") {
                    registered.store(true, Ordering::SeqCst);
                }
                let body = b"{}";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn killed_peer_mid_spawn_yields_structured_failure_not_a_panic_or_hang() {
        let peer_url = spawn_dying_peer();
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("build test http client");

        let result = perform_mesh_relay_call(
            &http,
            &peer_url,
            "signed-test-token",
            "blackstar",
            "POST",
            "/api/agents/spawn",
            Some(r#"{"agent":"claude","report_to":"main"}"#),
        );

        let failure =
            result.expect_err("a peer that dies mid-request must never look like a success");
        assert_eq!(failure.peer, "blackstar");
        assert!(
            !failure.reason.trim().is_empty(),
            "reason must name what happened, not be blank"
        );
        assert!(
            !failure.retry_hint.trim().is_empty(),
            "caller needs a next step, not just an error"
        );
    }

    #[test]
    fn killed_peer_mid_spawn_leaves_zero_residual_origin_bindings() {
        let registered = Arc::new(AtomicBool::new(false));
        let origin_base = spawn_origin_recording_registration(registered.clone());
        let peer_url = spawn_dying_peer();
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("build test http client");
        let spawn_body = serde_json::json!({"agent": "claude", "report_to": "main"});
        let origin_token = Some("origin-test-token".to_string());

        // Calls the SAME production function `mesh_spawn_and_register` uses —
        // a regression that registers unconditionally would flip `registered`.
        let outcome = spawn_and_maybe_register_at_resolved_peer(
            &http,
            &peer_url,
            "signed-test-token",
            "blackstar",
            &origin_base,
            &origin_token,
            &spawn_body,
            Some(&spawn_body.to_string()),
        );

        assert!(
            outcome.is_err(),
            "the dying peer must not look like a success"
        );
        assert!(
            !registered.load(Ordering::SeqCst),
            "a mesh spawn that never produced a live remote Tab must leave \
             zero residual task/lane bindings on the origin"
        );
    }
}
