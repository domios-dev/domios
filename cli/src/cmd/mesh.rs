use crate::{
    build_local_api, client::SocketClient, mesh_cli, read_api_config, MeshAction, MeshPeerAction,
};

fn handle_resp(resp: crate::local_api::LocalApiResponse) -> Result<serde_json::Value, String> {
    let status = resp.status();
    parse_mesh_response(status, &resp.text().unwrap_or_default())
}

fn parse_mesh_response(
    status: reqwest::StatusCode,
    body: &str,
) -> Result<serde_json::Value, String> {
    if status.is_success() {
        serde_json::from_str(body).map_err(|e| format!("Bad JSON: {}", e))
    } else {
        Err(format!("HTTP {} — {}", status.as_u16(), body.trim()))
    }
}

pub fn handle_mesh(client: &mut SocketClient, json: bool, action: MeshAction) {
    let (base, token) = read_api_config();
    let http = match build_local_api() {
        Ok(c) => c,
        Err(e) => {
            crate::cmd::print::fail(1, format!("failed to build HTTP client: {}", e));
        }
    };
    match action {
        MeshAction::Status => match client.call("mesh.status", None) {
            Ok(mut val) => {
                let account = http
                    .get(format!("{}/api/account/mesh", base))
                    .header(
                        "Authorization",
                        format!("Bearer {}", token.as_deref().unwrap_or("")),
                    )
                    .send()
                    .map_err(|error| error.to_string())
                    .and_then(handle_resp);
                val["account_mesh"] = match account {
                    Ok(state) => state,
                    Err(error) => serde_json::json!({ "status_error": error }),
                };
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                } else {
                    let name = val
                        .get("instance_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let id = val
                        .get("instance_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let paired = val
                        .get("paired_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    println!("{} ({})", name, &id[..id.len().min(8)]);
                    println!("Paired: {}", paired);
                    for line in account_mesh_status_lines(&val["account_mesh"]) {
                        println!("{}", line);
                    }
                }
            }
            Err(e) => {
                crate::cmd::print::fail(1, e.to_string());
            }
        },
        MeshAction::Peers => match client.call("mesh.peers", None) {
            Ok(val) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                } else {
                    if let Some(paired) = val.get("paired").and_then(|v| v.as_array()) {
                        if !paired.is_empty() {
                            println!("Paired:");
                        }
                        for p in paired {
                            let name = p
                                .get("instance_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?");
                            let online = p.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
                            let ip = p
                                .get("addresses")
                                .and_then(|v| v.as_array())
                                .and_then(|a| a.first())
                                .and_then(|a| a.get("ip"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("?");
                            let dot = if online { "●" } else { "○" };
                            println!(
                                "  {} {} ({}) {}",
                                dot,
                                name,
                                ip,
                                if p.get("is_primary")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false)
                                {
                                    "★"
                                } else {
                                    ""
                                }
                            );
                        }
                    }
                }
            }
            Err(e) => {
                crate::cmd::print::fail(1, e.to_string());
            }
        },
        MeshAction::Tabs { target } => {
            mesh_cli::mesh_relay_cmd(&target, "tabs", "", json);
        }
        MeshAction::Link { installation_id } => {
            let body = serde_json::json!({"installation_id": installation_id});
            let result = http
                .post(format!("{}/api/mesh/link", base))
                .header(
                    "Authorization",
                    format!("Bearer {}", token.as_deref().unwrap_or("")),
                )
                .json(&body)
                .send()
                .map_err(|e| e.to_string())
                .and_then(handle_resp);
            match result {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!(
                            "Waiting for approval on {}. Check Mesh status for the result.",
                            installation_id
                        );
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        MeshAction::Disconnect { instance_id } => {
            let body = serde_json::json!({"instance_id": instance_id});
            let result = http
                .post(format!("{}/api/mesh/disconnect", base))
                .header(
                    "Authorization",
                    format!("Bearer {}", token.as_deref().unwrap_or("")),
                )
                .json(&body)
                .send()
                .map_err(|e| e.to_string())
                .and_then(handle_resp);
            match result {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Disconnected from {}", instance_id);
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        MeshAction::Unrevoke { instance_id } => {
            let body = serde_json::json!({"instance_id": instance_id});
            let result = http
                .post(format!("{}/api/mesh/unrevoke", base))
                .header(
                    "Authorization",
                    format!("Bearer {}", token.as_deref().unwrap_or("")),
                )
                .json(&body)
                .send()
                .map_err(|e| e.to_string())
                .and_then(handle_resp);
            match result {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Revocation lifted for {}", instance_id);
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        MeshAction::Refresh => match http.post(format!("{}/api/mesh/refresh", base)).send() {
            Ok(resp) if resp.status().is_success() => {
                let val: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                } else {
                    let ts = val
                        .get("tailscale_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(none)");
                    let lan = val
                        .get("local_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(none)");
                    println!("Local addresses re-detected:");
                    println!("  tailscale: {}", ts);
                    println!("  lan:       {}", lan);
                    if let Some(rep) = val.get("announce") {
                        let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                        let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!("Announced to {}/{} paired peers", ok, sent);
                        if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                            if !stale.is_empty() {
                                println!("Stale peers (need manual update — too old to recognize announce):");
                                for s in stale {
                                    println!("  - {}", s.as_str().unwrap_or("?"));
                                }
                            }
                        }
                        if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                            if !errs.is_empty() {
                                println!("Errors:");
                                for e in errs {
                                    println!("  - {}", e.as_str().unwrap_or("?"));
                                }
                            }
                        }
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                crate::cmd::print::fail_http(1, status.as_u16(), &body);
            }
            Err(e) => {
                crate::cmd::print::fail(1, e.to_string());
            }
        },
        MeshAction::Announce { to } => {
            let url = match to {
                Some(t) => format!(
                    "{}/api/mesh/announce/all?to={}",
                    base,
                    urlencoding::encode(&t)
                ),
                None => format!("{}/api/mesh/announce/all", base),
            };
            match http.post(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let rep = val
                            .get("report")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                        let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!("Announced to {}/{} paired peers", ok, sent);
                        if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                            if !stale.is_empty() {
                                println!("Stale peers (need manual update):");
                                for s in stale {
                                    println!("  - {}", s.as_str().unwrap_or("?"));
                                }
                            }
                        }
                        if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                            if !errs.is_empty() {
                                println!("Errors:");
                                for e in errs {
                                    println!("  - {}", e.as_str().unwrap_or("?"));
                                }
                            }
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    crate::cmd::print::fail_http(1, status.as_u16(), &body);
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        MeshAction::Peer { action } => match action {
            MeshPeerAction::SetAddr { peer, addr } => {
                let (ip, port) = mesh_cli::parse_ip_port(&addr);
                if ip.is_empty() {
                    crate::cmd::print::fail(1, format!("invalid address '{}'", addr));
                }
                let mut body = serde_json::json!({"ip": ip});
                if let Some(p) = port {
                    body["port"] = serde_json::json!(p);
                }
                let url = format!(
                    "{}/api/mesh/peers/{}/addr",
                    base,
                    urlencoding::encode(&peer)
                );
                match http.post(&url).json(&body).send() {
                    Ok(resp) if resp.status().is_success() => {
                        let val: serde_json::Value = resp.json().unwrap_or_default();
                        if json {
                            println!("{}", serde_json::to_string_pretty(&val).unwrap());
                        } else {
                            let prev = val
                                .get("previous")
                                .and_then(|v| v.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|x| x.get("ip").and_then(|v| v.as_str()))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                })
                                .unwrap_or_default();
                            println!(
                                "Set {} → {}{}",
                                peer,
                                ip,
                                port.map(|p| format!(":{}", p)).unwrap_or_default()
                            );
                            if !prev.is_empty() {
                                println!("  was: {}", prev);
                            }
                        }
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().unwrap_or_default();
                        crate::cmd::print::fail_http(1, status.as_u16(), &body);
                    }
                    Err(e) => {
                        crate::cmd::print::fail(1, e.to_string());
                    }
                }
            }
        },
        MeshAction::Offline => set_mesh_enabled(&http, &base, &token, false, json),
        MeshAction::Online => set_mesh_enabled(&http, &base, &token, true, json),
    }
}

#[cfg(test)]
#[path = "mesh/tests.rs"]
mod tests;

/// Flip `[mesh] enabled` in config.toml via the existing PATCH /api/config
/// endpoint. Both the address-refresh loop and the ping scheduler re-read
/// the flag on every tick, so the change takes effect within ~15s without
/// a daemon restart. The local Tailscale daemon is left untouched.
fn set_mesh_enabled(
    http: &crate::local_api::LocalApiClient,
    base: &str,
    token: &Option<String>,
    enabled: bool,
    json: bool,
) {
    // instance_name, etc.
    let body = serde_json::json!({"key": "mesh.enabled", "value": enabled});
    let resp = http
        .patch(format!("{}/api/config", base))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .json(&body)
        .send();
    match resp {
        Ok(r) if r.status().is_success() => {
            if json {
                let val: serde_json::Value = r.json().unwrap_or_default();
                println!("{}", serde_json::to_string_pretty(&val).unwrap());
            } else if enabled {
                println!("mesh: ONLINE — peers will see us within ~15s");
            } else {
                println!("mesh: OFFLINE — invisible to peers; Tailscale untouched");
            }
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().unwrap_or_default();
            crate::cmd::print::fail_http(1, status.as_u16(), &body);
        }
        Err(e) => {
            crate::cmd::print::fail(1, e.to_string());
        }
    }
}

fn account_mesh_status_lines(state: &serde_json::Value) -> Vec<String> {
    if state["last_refusal"]["reason"] == "reauth_required" {
        return vec!["Sign in again to add this machine to your mesh".into()];
    }
    let mut lines = Vec::new();
    if let Some(publication) = state["publication"].as_str() {
        lines.push(format!("Account publication: {publication}"));
    }
    for field in ["publication_failure", "last_refusal", "status_error"] {
        if !state[field].is_null() {
            lines.push(format!("Account {field}: {}", state[field]));
        }
    }
    if let Some(at) = state["retry_at"].as_u64() {
        lines.push(format!(
            "Account mesh: waiting to retry at {at} (Unix seconds)"
        ));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Some(rows) = state["registry"]["rows"].as_array() {
        for row in rows {
            let name = row["display_name"]
                .as_str()
                .or_else(|| row["installation_id"].as_str())
                .unwrap_or("Unnamed machine");
            lines.push(format!("  {name}: {}", account_machine_status(row, now)));
        }
    }
    lines
}

fn account_machine_status(row: &serde_json::Value, now: u64) -> &'static str {
    if row["active"] == false || row["eligibility"] == "revoked" {
        "Removed"
    } else if row["presence"] == "signed_out" {
        "Signed out (actions disabled)"
    } else if row["membership_expires_at"]
        .as_u64()
        .is_some_and(|at| at <= now)
    {
        "Membership expired"
    } else if row["endpoint"]["expires_at"]
        .as_u64()
        .is_none_or(|at| at <= now)
    {
        "Offline"
    } else {
        "Present"
    }
}
