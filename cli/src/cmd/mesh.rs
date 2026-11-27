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
        MeshAction::Identity {
            action: mesh_cli::MeshIdentityAction::Set { kind, peer },
        } => {
            // Keep sending the neutral legacy color until 1.23.2 peers leave the fleet.
            let identity = serde_json::json!({ "kind": kind, "color": "graphite" });
            let path = "/api/account/mesh/identity";
            let result = if let Some(peer) = peer {
                mesh_cli::mesh_relay_fetch_json(
                    &peer,
                    "POST",
                    "/api/mesh/identity",
                    Some(&identity.to_string()),
                )
            } else {
                let response = http
                    .post(format!("{base}{path}"))
                    .header(
                        "Authorization",
                        format!("Bearer {}", token.as_deref().unwrap_or("")),
                    )
                    .json(&identity)
                    .send()
                    .map_err(|error| error.to_string())
                    .and_then(handle_resp);
                match response {
                    Ok(result) => result,
                    Err(error) => crate::cmd::print::fail(1, error),
                }
            };
            if json {
                crate::cmd::print::print_json(&result);
            } else if result["state"] == "awaiting_owner" {
                println!(
                    "Awaiting owner approval ({}).",
                    result["notification_id"].as_str().unwrap_or("notification")
                );
            } else {
                println!("Machine identity saved.");
            }
        }
        MeshAction::Status => match client.call("mesh.status", None) {
            Ok(mut val) => {
                let account = http
                    .get(format!("{}/api/account/mesh/summary", base))
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
                    println!("{}", crate::cmd::output::encode(&val).unwrap());
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
                    println!("Account connections: {}", paired);
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
                    println!("{}", crate::cmd::output::encode(&val).unwrap());
                } else {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    if let Some(rows) = val.get("installations").and_then(|rows| rows.as_array()) {
                        for row in rows {
                            if row["active"] == false {
                                continue;
                            }
                            let name = row["name"].as_str().unwrap_or("Unnamed machine");
                            let id = row["installation_id"].as_str().unwrap_or("?");
                            let status = if row["active"] == false {
                                "Removed"
                            } else if row["signed_out"] == true {
                                "Sign in on this machine"
                            } else if row["membership_expires_at"]
                                .as_u64()
                                .is_some_and(|at| at <= now)
                            {
                                "Membership expired"
                            } else if val["direct_ingress"][id]["stream_attached"] == false {
                                "Reachable only while one of your machines is online"
                            } else if val["direct_ingress"][id]["stream_attached"] == true
                                || row["online"] == true
                            {
                                "Connected"
                            } else if row["endpoint_expires_at"]
                                .as_u64()
                                .is_none_or(|at| at <= now)
                            {
                                "Stale endpoint"
                            } else {
                                "Connecting automatically"
                            };
                            let cloud_suffix = row
                                .get("cloud")
                                .filter(|c| c.is_object())
                                .map(|c| {
                                    format!(" [{}]", mesh_cli::cloud::format_cloud_cost_badge(c))
                                })
                                .unwrap_or_default();
                            println!("  {name} ({id}): {status}{cloud_suffix}");
                        }
                    }
                    if let Some(clouds) = val
                        .get("cloud_machines")
                        .and_then(|rows| rows.as_array())
                        .filter(|rows| !rows.is_empty())
                    {
                        println!(
                            "Cloud machines ({} — these machines cost money while awake):",
                            clouds.len()
                        );
                        for cloud in clouds {
                            let cname = cloud["name"].as_str().unwrap_or("?");
                            let cid = cloud["machine_id"].as_str().unwrap_or("?");
                            println!(
                                "  {cname} ({cid}): {}",
                                mesh_cli::cloud::format_cloud_cost_badge(cloud)
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
        MeshAction::Refresh => match http.post(format!("{}/api/mesh/refresh", base)).send() {
            Ok(resp) if resp.status().is_success() => {
                let val: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", crate::cmd::output::encode(&val).unwrap());
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
                        println!("{}", crate::cmd::output::encode(&val).unwrap());
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
                            println!("{}", crate::cmd::output::encode(&val).unwrap());
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
        MeshAction::Remove {
            machine,
            stale,
            older_than,
            dry_run,
        } => {
            if stale {
                remove_stale_mesh_machines(&http, &base, &token, older_than, dry_run, json);
            } else if let Some(machine) = machine {
                remove_mesh_machine(&http, &base, &token, &machine, json);
            } else {
                crate::cmd::print::fail(1, "specify a target machine or pass --stale");
            }
        }
        MeshAction::Cloud { action } => {
            mesh_cli::cloud::handle_cloud(&http, &base, &token, json, action)
        }
    }
}

fn remove_mesh_machine(
    http: &crate::local_api::LocalApiClient,
    base: &str,
    token: &Option<String>,
    machine: &str,
    json: bool,
) {
    let body = serde_json::json!({ "machine": machine });
    let resp = http
        .post(format!("{base}/api/account/mesh/remove"))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .json(&body)
        .send();
    match resp {
        Ok(resp) if resp.status().is_success() => {
            let val: serde_json::Value = match resp.json() {
                Ok(val) => val,
                Err(error) => crate::cmd::print::fail(1, format!("Bad JSON: {error}")),
            };
            if val.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                crate::cmd::print::fail_http(1, 400, &val.to_string());
            }
            let result = val.get("result").cloned().unwrap_or(val);
            if json {
                crate::cmd::print::print_json(&result);
            } else {
                let id = result["installation_id"].as_str().unwrap_or(machine);
                let name = result["display_name"].as_str().unwrap_or(id);
                let signed_out = result["signed_out"].as_bool().unwrap_or(false);
                if signed_out {
                    println!("Removed {name} ({id}) from the account mesh (signed out first).");
                } else {
                    println!("Removed {name} ({id}) from the account mesh.");
                }
            }
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            crate::cmd::print::fail_http(1, status.as_u16(), &body);
        }
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    }
}

fn remove_stale_mesh_machines(
    http: &crate::local_api::LocalApiClient,
    base: &str,
    token: &Option<String>,
    older_than_days: Option<u64>,
    dry_run: bool,
    json: bool,
) {
    let mut body = serde_json::json!({ "dry_run": dry_run });
    if let Some(days) = older_than_days {
        body["older_than_days"] = serde_json::json!(days);
    }
    let resp = http
        .post(format!("{base}/api/account/mesh/remove-stale"))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .json(&body)
        .send();
    match resp {
        Ok(resp) if resp.status().is_success() => {
            let val: serde_json::Value = match resp.json() {
                Ok(val) => val,
                Err(error) => crate::cmd::print::fail(1, format!("Bad JSON: {error}")),
            };
            if val.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                crate::cmd::print::fail_http(1, 400, &val.to_string());
            }
            let result = val.get("result").cloned().unwrap_or(val);
            if json {
                crate::cmd::print::print_json(&result);
            } else {
                for line in format_remove_stale_lines(&result) {
                    println!("{line}");
                }
            }
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            crate::cmd::print::fail_http(1, status.as_u16(), &body);
        }
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    }
}

fn stale_reason_label(reason: Option<&str>) -> Option<&'static str> {
    match reason? {
        "signed_out" => Some("signed out"),
        "membership_expired" => Some("membership expired"),
        "unreachable" => Some("unreachable"),
        _ => None,
    }
}

fn stale_status_label(status: &str) -> &'static str {
    match status {
        "would_remove" => "would remove",
        "removed" => "removed",
        "already_removed" => "already removed",
        "refused_self" => "skipped (this machine)",
        "refused_online" => "skipped (online)",
        "not_found" => "not found",
        _ => "unknown",
    }
}

pub(crate) fn format_remove_stale_lines(result: &serde_json::Value) -> Vec<String> {
    let dry_run = result["dry_run"].as_bool().unwrap_or(false);
    let rows = result["rows"].as_array();
    let matched = result["matched"]
        .as_u64()
        .unwrap_or_else(|| rows.map_or(0, |r| r.len() as u64));
    if matched == 0 {
        return vec!["No stale machines found.".into()];
    }
    let noun = |n: u64| if n == 1 { "machine" } else { "machines" };
    let mut lines = Vec::new();
    if dry_run {
        lines.push(format!(
            "Dry run: {matched} stale {} would be removed:",
            noun(matched)
        ));
    } else {
        let removed = result["removed"].as_u64().unwrap_or(0);
        let already_removed = result["already_removed"].as_u64().unwrap_or(0);
        let skipped = result["skipped"].as_u64().unwrap_or(0);
        let mut extras = Vec::new();
        if already_removed > 0 {
            extras.push(format!("{already_removed} already removed"));
        }
        if skipped > 0 {
            extras.push(format!("{skipped} skipped"));
        }
        if extras.is_empty() {
            lines.push(format!("Removed {removed} stale {}:", noun(removed)));
        } else {
            lines.push(format!(
                "Removed {removed} stale {} ({}):",
                noun(removed),
                extras.join(", ")
            ));
        }
    }
    if let Some(rows) = rows {
        for row in rows {
            let id = row["installation_id"].as_str().unwrap_or("?");
            let name = row["display_name"].as_str().unwrap_or(id);
            let status = stale_status_label(row["status"].as_str().unwrap_or(""));
            let reason = stale_reason_label(row["reason"].as_str());
            let detail = match reason {
                Some(reason) => format!("{status} ({reason})"),
                None => status.to_owned(),
            };
            lines.push(format!("  {name} ({id}): {detail}"));
        }
    }
    lines
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
                println!("{}", crate::cmd::output::encode(&val).unwrap());
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
    if state["last_refusal"]["reason"] == "proof_invalid" {
        lines.push(
            "This build cannot join this account's mesh: the account service rejected its proof"
                .into(),
        );
    }
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
    let local_id = state["local_installation_id"].as_str();
    let local_published = state["publication"].as_str() == Some("published");
    let local_membership_expiry = state["membership_expires_at"].as_u64();
    let any_pair_online = state["pairs"]
        .as_array()
        .is_some_and(|pairs| pairs.iter().any(|pair| pair["online"] == true));
    if let Some(rows) = state["registry"]["rows"].as_array() {
        for row in rows {
            if row["active"] == false || row["eligibility"] == "revoked" {
                continue;
            }
            let installation_id = row["installation_id"].as_str();
            let pair_online = installation_id.is_some_and(|id| {
                state["pairs"].as_array().is_some_and(|pairs| {
                    pairs.iter().any(|pair| {
                        pair["installation_id"].as_str() == Some(id) && pair["online"] == true
                    })
                })
            });
            let is_local = installation_id.is_some_and(|id| local_id == Some(id));
            let local_online = is_local && (local_published || any_pair_online);
            let name = row["display_name"]
                .as_str()
                .or(installation_id)
                .unwrap_or("Unnamed machine");
            lines.push(format!(
                "  {name}: {}",
                account_machine_status_with_live(
                    row,
                    now,
                    pair_online,
                    local_online,
                    is_local.then_some(local_membership_expiry).flatten(),
                )
            ));
        }
    }
    lines
}

#[cfg(test)]
fn account_machine_status(row: &serde_json::Value, now: u64) -> &'static str {
    account_machine_status_with_live(row, now, false, false, None)
}

fn account_machine_status_with_live(
    row: &serde_json::Value,
    now: u64,
    pair_online: bool,
    local_online: bool,
    local_membership_expiry: Option<u64>,
) -> &'static str {
    let membership_expires_at = match (
        row["membership_expires_at"].as_u64(),
        local_membership_expiry,
    ) {
        (Some(row_exp), Some(local_exp)) => Some(row_exp.max(local_exp)),
        (Some(row_exp), None) => Some(row_exp),
        (None, Some(local_exp)) => Some(local_exp),
        (None, None) => None,
    };
    if row["active"] == false || row["eligibility"] == "revoked" {
        "Removed"
    } else if row["presence"] == "signed_out" {
        "Signed out (actions disabled)"
    } else if membership_expires_at.is_some_and(|at| at <= now) {
        "Membership expired"
    } else if pair_online {
        "Connected"
    } else if local_online {
        "This machine"
    } else if row["endpoint"]["expires_at"]
        .as_u64()
        .is_none_or(|at| at <= now)
    {
        "Offline"
    } else {
        "Present"
    }
}
