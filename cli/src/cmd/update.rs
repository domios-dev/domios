use crate::cmd::print::print_self_update_outcome;
use crate::{build_http, read_api_config, UpdateAction};

pub fn handle_update(
    json: bool,
    action: Option<UpdateAction>,
    check: bool,
    apply: bool,
    channel: Option<String>,
    manifest_url: Option<String>,
) {
    let (base_url, token) = read_api_config();
    let http = build_http(&token);

    let resolved = if let Some(ch) = channel {
        Some(UpdateAction::Channel {
            channel: if ch.is_empty() { None } else { Some(ch) },
        })
    } else if apply {
        Some(UpdateAction::Install {
            auto: true,
            manifest_url: manifest_url.clone(),
            from_file: None,
            continue_prompt: None,
            continue_pane: None,
        })
    } else if check {
        Some(UpdateAction::Check { force: false })
    } else {
        action
    };

    match resolved.unwrap_or(UpdateAction::Check { force: false }) {
        UpdateAction::Check { force } => {
            let url = if force {
                format!("{}/api/update/check?force=true", base_url)
            } else {
                format!("{}/api/update/check", base_url)
            };
            match http.get(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                    } else {
                        let current = body["current_version"].as_str().unwrap_or("?");
                        let latest = body["latest_version"].as_str().unwrap_or("?");
                        let method = body["install_method"].as_str().unwrap_or("unknown");
                        let available = body["update_available"].as_bool().unwrap_or(false);

                        println!("Current: v{}  Latest: v{}  Install: {}", current, latest, method);
                        if available {
                            println!("Update available!");
                            if let Some(cmd) = body["update_command"].as_str() {
                                println!("Run: {}", cmd);
                            } else if let Some(url) = body["download_url"].as_str() {
                                println!("Download: {}", url);
                            }
                        } else {
                            println!("Up to date.");
                        }
                    }
                }
                Ok(resp) => {
                    eprintln!("Error: {}", resp.status());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        UpdateAction::Install { auto, manifest_url, from_file, continue_prompt, continue_pane } => {
            if let Some(path) = from_file {
                let pane = continue_pane
                    .or_else(|| std::env::var("CODETERM_PANE_ID").ok());
                crate::cmd::install::run_install_from_file(&path, json, continue_prompt.as_deref(), pane.as_deref());
                return;
            }
            let mut params: Vec<(&str, String)> = Vec::new();
            if auto {
                params.push(("method", "auto".into()));
            }
            if let Some(m) = manifest_url {
                params.push(("manifest_url", m));
            }
            let qs = if params.is_empty() {
                String::new()
            } else {
                let pairs: Vec<String> = params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                    .collect();
                format!("?{}", pairs.join("&"))
            };
            let url = format!("{}/api/update/install{}", base_url, qs);
            match http.post(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                    } else {
                        let status = body["status"].as_str().unwrap_or("unknown");
                        match status {
                            "up_to_date" => println!("Already up to date."),
                            "command" => {
                                if let Some(cmd) = body["command"].as_str() {
                                    println!("{}", cmd);
                                }
                            }
                            "browser_opened" => {
                                println!("Opened download page in browser.");
                            }
                            "updating" => {
                                if let Some(msg) = body["message"].as_str() {
                                    println!("{}", msg);
                                }
                            }
                            _ => println!("{}", body),
                        }
                    }
                }
                Ok(resp) => {
                    eprintln!("Error: {}", resp.status());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        UpdateAction::Channel { channel } => {
            let url = format!("{}/api/update/channel", base_url);
            let result = match channel {
                None => http.get(&url).send(),
                Some(name) => http
                    .post(&url)
                    .json(&serde_json::json!({ "channel": name }))
                    .send(),
            };
            match result {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                    } else if let Some(c) = body["channel"].as_str() {
                        println!("Channel: {}", c);
                    } else {
                        println!("{}", body);
                    }
                }
                Ok(resp) => {
                    eprintln!("Error: {}", resp.status());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

pub fn handle_self_update(json: bool, check: bool, apply: bool, rollback: bool, channel: Option<String>) {
    let (base_url, token) = read_api_config();
    let http = build_http(&token);

    if check {
        let url = format!("{}/api/update/check", base_url);
        match http.get(&url).send() {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                } else {
                    let current = body["current_version"].as_str().unwrap_or("?");
                    let latest = body["latest_version"].as_str().unwrap_or("?");
                    let available = body["update_available"].as_bool().unwrap_or(false);
                    println!("Current: v{}  Latest: v{}", current, latest);
                    println!("{}", if available { "Update available." } else { "Up to date." });
                }
            }
            Ok(resp) => { eprintln!("Error: {}", resp.status()); std::process::exit(1); }
            Err(e)   => { eprintln!("Error: {}", e); std::process::exit(1); }
        }
        return;
    }

    if rollback {
        let url = format!("{}/api/update/rollback", base_url);
        match http.post(&url).send() {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                } else {
                    print_self_update_outcome(&body);
                }
            }
            Ok(resp) => { eprintln!("Error: {}", resp.status()); std::process::exit(1); }
            Err(e)   => { eprintln!("Error: {}", e); std::process::exit(1); }
        }
        return;
    }

    let _ = apply;
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(c) = channel { params.push(("channel", c)); }
    let qs = if params.is_empty() {
        String::new()
    } else {
        let pairs: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect();
        format!("?{}", pairs.join("&"))
    };
    let url = format!("{}/api/update/self_apply{}", base_url, qs);
    match http.post(&url).send() {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
            } else {
                print_self_update_outcome(&body);
            }
        }
        Ok(resp) => { eprintln!("Error: {}", resp.status()); std::process::exit(1); }
        Err(e)   => { eprintln!("Error: {}", e); std::process::exit(1); }
    }
}
