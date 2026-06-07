//! Relay local API calls to a paired peer via signed /api/mesh/relay POSTs.

use crate::data_dir_name;

/// Resolve a peer's API URL by name or instance_id via the local /api/mesh/peers.
pub(crate) fn resolve_peer_url(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
) -> Option<String> {
    let peers_resp = http.get(format!("{}/api/mesh/peers", base))
        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
        .send().ok().and_then(|r| r.json::<serde_json::Value>().ok())?;
    let paired = peers_resp.get("paired").and_then(|v| v.as_array())?;
    let peer = paired.iter().find(|p| {
        p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
            || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
    })?;
    let ip = peer.get("addresses").and_then(|v| v.as_array())
        .and_then(|a| a.first()).and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
    let port = peer.get("api_port").and_then(|v| v.as_u64()).unwrap_or(7685);
    Some(format!("http://{}:{}", ip, port))
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
    let relay = serde_json::json!({
        "method": method,
        "path": path,
        "body": body.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()).unwrap_or(serde_json::json!(null)),
    });
    let resp = http.post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay).send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
        format!("HTTP {} {}: {}", status.as_u16(), status.canonical_reason().unwrap_or(""), preview.trim())
    })
}

/// Shortcut relay — maps old-style actions to new generic relay format.
pub fn mesh_relay_cmd(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, action: &str, payload: &str, pane: &str, json_mode: bool) {
    let (method, path, body) = match action {
        "exec" | "send" => {
            let source = std::env::var("CODETERM_PANE_ID").unwrap_or_else(|_| "unknown".to_string());
            ("POST", format!("/api/panes/{}/send", if pane.is_empty() { "general" } else { pane }),
                serde_json::json!({"text": payload, "enter": true, "source_pane": source}))
        }
        "output" => ("GET", format!("/api/panes/{}/output", if pane.is_empty() { "general" } else { pane }),
            serde_json::json!(null)),
        "panes" => ("GET", "/api/panes".to_string(), serde_json::json!(null)),
        "status" => ("GET", "/api/mesh/status".to_string(), serde_json::json!(null)),
        _ => { eprintln!("Unknown action: {}", action); std::process::exit(1); }
    };
    let body_str = if body.is_null() { None } else { Some(body.to_string()) };
    mesh_relay_generic(http, base, token, target, method, &path, body_str.as_deref(), json_mode);
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
                dirs::home_dir().unwrap_or_default().join(data_dir_name()).join("mesh/identity.json")
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
        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
    }
}

/// Generic relay — send any API call through mesh to a remote peer.
pub fn mesh_relay_generic(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, method: &str, path: &str, body: Option<&str>, json_mode: bool) {
    let mut peers_json = fetch_peers(http, base, token);
    let mut peer = find_peer(&peers_json, target).cloned();
    if peer.is_none() {
        let _ = http.post(format!("{}/api/mesh/refresh", base))
            .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
            .send();
        peers_json = fetch_peers(http, base, token);
        peer = find_peer(&peers_json, target).cloned();
    }
    let Some(peer) = peer else {
        eprintln!("Peer '{}' not found (after refresh)", target);
        std::process::exit(1);
    };
    let ip = peer.get("addresses").and_then(|v| v.as_array())
        .and_then(|a| a.first()).and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
    let port = peer.get("api_port").and_then(|v| v.as_u64()).unwrap_or(7685);
    let peer_url = format!("http://{}:{}", ip, port);

    // Mesh peers require a freshly signed Ed25519 token from mesh identity.
    // Local CODETERM_API_TOKEN is not accepted by peer verifiers.
    let signed_token = match crate::read_mesh_signed_token() {
        Some(t) => t,
        None => {
            eprintln!(
                "Error: cannot sign mesh request — no identity at {:?}.",
                dirs::home_dir().unwrap_or_default().join(data_dir_name()).join("mesh/identity.json")
            );
            std::process::exit(1);
        }
    };

    let relay = serde_json::json!({
        "method": method,
        "path": path,
        "body": body.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()).unwrap_or(serde_json::json!(null)),
    });

    match http.post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay).send() {
        Ok(resp) => {
            let status = resp.status();
            let bytes = resp.bytes().unwrap_or_default();
            let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();

            if !status.is_success() && val.is_null() {
                let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
                eprintln!("Error: peer returned HTTP {} ({}): {}", status.as_u16(), peer_url, preview.trim());
                std::process::exit(1);
            }

            // Relay returns 200+ok:false on auth/HTTP failures; surface as errors.
            if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                if val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            }

            let data = val.get("data").cloned().unwrap_or(serde_json::Value::Null);

            if json_mode {
                // Unwrap envelope so `--json` output matches local CLI shape.
                println!("{}", serde_json::to_string_pretty(&data).unwrap());
            } else if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
                // /panes/<id>/output returns {"text": "..."} — print raw.
                print!("{}", text);
            } else if data.is_null() {
                let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
                eprintln!("Error: unexpected response from peer (HTTP {}): {}", status.as_u16(), preview.trim());
                std::process::exit(1);
            } else {
                crate::cmd::print::print_human(&data);
            }
        }
        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
    }
}

fn fetch_peers(http: &reqwest::blocking::Client, base: &str, token: &Option<String>) -> serde_json::Value {
    http.get(format!("{}/api/mesh/peers", base))
        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
        .send().ok().and_then(|r| r.json::<serde_json::Value>().ok())
        .unwrap_or(serde_json::json!({}))
}

fn find_peer<'a>(peers_json: &'a serde_json::Value, target: &str) -> Option<&'a serde_json::Value> {
    peers_json.get("paired").and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|p| {
            p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
            || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
        }))
}
