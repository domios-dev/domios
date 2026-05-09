use crate::{build_http, mesh_cli, read_api_config, MeshAction, MeshPeerAction};

fn handle_resp(
    resp: reqwest::blocking::Response,
) -> Result<serde_json::Value, String> {
    let status = resp.status();
    if status.is_success() {
        resp.json::<serde_json::Value>()
            .map_err(|e| format!("Bad JSON: {}", e))
    } else {
        let body = resp.text().unwrap_or_default();
        Err(format!("HTTP {} — {}", status.as_u16(), body.trim()))
    }
}

pub fn handle_mesh(json: bool, action: MeshAction) {
    let (base, token) = read_api_config();
    let http = build_http(&token);
    match action {
        MeshAction::Status => {
            match http.get(format!("{}/api/mesh/status", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let name = val.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                        let id = val.get("instance_id").and_then(|v| v.as_str()).unwrap_or("?");
                        let paired = val.get("paired_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        let discovered = val.get("discovered_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!("{} ({})", name, &id[..id.len().min(8)]);
                        println!("Paired: {}, Discovered: {}", paired, discovered);
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Peers => {
            match http.get(format!("{}/api/mesh/peers", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        if let Some(paired) = val.get("paired").and_then(|v| v.as_array()) {
                            if !paired.is_empty() { println!("Paired:"); }
                            for p in paired {
                                let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                let online = p.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
                                let ip = p.get("addresses").and_then(|v| v.as_array())
                                    .and_then(|a| a.first())
                                    .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                let dot = if online { "●" } else { "○" };
                                println!("  {} {} ({}) {}", dot, name, ip,
                                    if p.get("is_primary").and_then(|v| v.as_bool()).unwrap_or(false) { "★" } else { "" });
                            }
                        }
                        if let Some(discovered) = val.get("discovered").and_then(|v| v.as_array()) {
                            if !discovered.is_empty() { println!("Discovered:"); }
                            for d in discovered {
                                let name = d.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                let method = d.get("discovery_method").and_then(|v| v.as_str()).unwrap_or("?");
                                let ip = d.get("addresses").and_then(|v| v.as_array())
                                    .and_then(|a| a.first())
                                    .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                let dev = d.get("dev").and_then(|v| v.as_bool()).unwrap_or(false);
                                println!("  ◇ {} ({}) [{}]{}", name, ip, method, if dev { " [dev]" } else { "" });
                            }
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Exec { target, command, pane } => {
            mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "exec", &command, &pane, json);
        }
        MeshAction::Send { target, text, pane } => {
            mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "send", &text, &pane, json);
        }
        MeshAction::Output { target, pane } => {
            mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "output", "", &pane, json);
        }
        MeshAction::Panes { target } => {
            mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "panes", "", "", json);
        }
        MeshAction::Connect { instance_id } => {
            let body = serde_json::json!({"instance_id": instance_id});
            let result = http.post(format!("{}/api/mesh/connect", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .json(&body).send()
                .map_err(|e| e.to_string())
                .and_then(handle_resp);
            match result {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Pairing request sent to {}", instance_id);
                        if let Some(rid) = val.get("request_id").and_then(|v| v.as_str()) {
                            println!("Request ID: {}", rid);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Approve { request_id } => {
            let body = serde_json::json!({"request_id": request_id});
            let result = http.post(format!("{}/api/mesh/approve", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .json(&body).send()
                .map_err(|e| e.to_string())
                .and_then(handle_resp);
            match result {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Pairing request {} approved", request_id);
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Reject { request_id } => {
            let body = serde_json::json!({"request_id": request_id});
            let result = http.post(format!("{}/api/mesh/reject", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .json(&body).send()
                .map_err(|e| e.to_string())
                .and_then(handle_resp);
            match result {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Pairing request {} rejected and blocked", request_id);
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Disconnect { instance_id } => {
            let body = serde_json::json!({"instance_id": instance_id});
            let result = http.post(format!("{}/api/mesh/disconnect", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .json(&body).send()
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
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Discover => {
            match http.post(format!("{}/api/mesh/discover", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No discovered peers");
                        } else {
                            for p in arr {
                                let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                let id = p.get("instance_id").and_then(|v| v.as_str()).unwrap_or("?");
                                let method = p.get("discovery_method").and_then(|v| v.as_str()).unwrap_or("?");
                                println!("  {} ({}) [{}]", name, id, method);
                            }
                        }
                    } else {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Broadcast => {
            match http.post(format!("{}/api/mesh/broadcast/start", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(dur) = val.get("duration_secs").and_then(|v| v.as_u64()) {
                        println!("Broadcasting for {}s", dur);
                    } else {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::StopBroadcast => {
            match http.post(format!("{}/api/mesh/broadcast/stop", base))
                .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                .send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Broadcast stopped");
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Refresh => {
            match http.post(format!("{}/api/mesh/refresh", base)).send() {
                Ok(resp) if resp.status().is_success() => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let ts = val.get("tailscale_ip").and_then(|v| v.as_str()).unwrap_or("(none)");
                        let lan = val.get("local_ip").and_then(|v| v.as_str()).unwrap_or("(none)");
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
                                    for s in stale { println!("  - {}", s.as_str().unwrap_or("?")); }
                                }
                            }
                            if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                                if !errs.is_empty() {
                                    println!("Errors:");
                                    for e in errs { println!("  - {}", e.as_str().unwrap_or("?")); }
                                }
                            }
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    eprintln!("Error ({}): {}", status, body);
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Announce { to } => {
            let url = match to {
                Some(t) => format!("{}/api/mesh/announce/all?to={}", base, urlencoding::encode(&t)),
                None => format!("{}/api/mesh/announce/all", base),
            };
            match http.post(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let rep = val.get("report").cloned().unwrap_or(serde_json::Value::Null);
                        let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                        let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!("Announced to {}/{} paired peers", ok, sent);
                        if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                            if !stale.is_empty() {
                                println!("Stale peers (need manual update):");
                                for s in stale { println!("  - {}", s.as_str().unwrap_or("?")); }
                            }
                        }
                        if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                            if !errs.is_empty() {
                                println!("Errors:");
                                for e in errs { println!("  - {}", e.as_str().unwrap_or("?")); }
                            }
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    eprintln!("Error ({}): {}", status, body);
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        MeshAction::Peer { action } => match action {
            MeshPeerAction::SetAddr { peer, addr } => {
                let (ip, port) = mesh_cli::parse_ip_port(&addr);
                if ip.is_empty() {
                    eprintln!("Error: invalid address '{}'", addr);
                    std::process::exit(1);
                }
                let mut body = serde_json::json!({"ip": ip});
                if let Some(p) = port { body["port"] = serde_json::json!(p); }
                let url = format!("{}/api/mesh/peers/{}/addr", base, urlencoding::encode(&peer));
                match http.post(&url).json(&body).send() {
                    Ok(resp) if resp.status().is_success() => {
                        let val: serde_json::Value = resp.json().unwrap_or_default();
                        if json {
                            println!("{}", serde_json::to_string_pretty(&val).unwrap());
                        } else {
                            let prev = val.get("previous").and_then(|v| v.as_array())
                                .map(|a| a.iter().filter_map(|x| x.get("ip").and_then(|v| v.as_str())).collect::<Vec<_>>().join(", "))
                                .unwrap_or_default();
                            println!("Set {} → {}{}",
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
                        eprintln!("Error ({}): {}", status, body);
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            }
        },
    }
}
