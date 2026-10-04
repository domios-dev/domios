use std::time::{Duration, Instant};

use crate::{build_local_api, read_api_config, UpdateAction};

const DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT: &str =
    "Check that the new version is installed, then continue the interrupted work.";

fn resolve_local_install_continuation(
    disabled: bool,
    prompt: Option<String>,
    tab: Option<String>,
    environment_tab: Option<String>,
) -> (Option<String>, Option<String>) {
    if disabled {
        return (None, None);
    }
    let tab = tab.or(environment_tab);
    let prompt = prompt.or_else(|| {
        tab.as_ref()
            .map(|_| DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT.to_string())
    });
    (prompt, tab)
}

#[derive(Debug, PartialEq, Eq)]
enum ContinuationRegistration {
    NotRequested,
    Registered,
    Refused(String),
    /// The running daemon predates the restart-continuation owner.
    Unsupported,
}

fn classify_continuation(
    tab: Option<&str>,
    continuation: &serde_json::Value,
) -> ContinuationRegistration {
    if tab.is_none() {
        return ContinuationRegistration::NotRequested;
    }
    match continuation["registered"].as_bool() {
        Some(true) => ContinuationRegistration::Registered,
        Some(false) => ContinuationRegistration::Refused(
            continuation["reason"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
        ),
        None => ContinuationRegistration::Unsupported,
    }
}

/// Persist the desk and hand the calling tab's continuation to the daemon that
/// restarts; the next start delivers it once that tab's session is agent-ready.
fn save_session_with_continuation(
    http: &crate::local_api::LocalApiClient,
    base_url: &str,
    prompt: Option<&str>,
    tab: Option<&str>,
) {
    let save_url = format!("{}/api/session/save", base_url);
    let request = http.post(&save_url);
    let request = match tab {
        Some(tab) => request.json(&serde_json::json!({
            "continue_tab": tab,
            "continue_prompt": prompt,
        })),
        None => request,
    };
    let saved = request
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.json::<serde_json::Value>());
    let saved = match saved {
        Ok(saved) => saved,
        Err(e) => {
            eprintln!("[install --from-file] WARN: session save failed ({e}) — Tab id may rotate");
            return;
        }
    };
    match classify_continuation(tab, &saved["continuation"]) {
        ContinuationRegistration::NotRequested => {}
        ContinuationRegistration::Registered => eprintln!(
            "[install --from-file] Continue: Domios continues tab {} once its session is ready after restart",
            tab.unwrap_or("?")
        ),
        ContinuationRegistration::Refused(reason) => eprintln!(
            "[install --from-file] WARN: continuation for tab {} refused: {reason}",
            tab.unwrap_or("?")
        ),
        ContinuationRegistration::Unsupported => eprintln!(
            "[install --from-file] WARN: running Domios predates restart continuation; tab {} is not continued",
            tab.unwrap_or("?")
        ),
    }
}

fn poll_update_status(http: &crate::local_api::LocalApiClient, base_url: &str) {
    let status_url = format!("{}/api/update/status", base_url);
    let started = Instant::now();
    let timeout = Duration::from_secs(300);
    let mut last_phase = String::new();
    let mut last_err = String::new();
    let mut connect_failures: u32 = 0;
    loop {
        if started.elapsed() > timeout {
            eprintln!("Timed out after {}s waiting for update to complete. Check ~/.codeterm/logs/codeterm.log for details.", timeout.as_secs());
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(500));
        match http.get(&status_url).send() {
            Ok(resp) if resp.status().is_success() => {
                connect_failures = 0;
                let body: serde_json::Value = resp.json().unwrap_or_default();
                let phase = body["phase"].as_str().unwrap_or("idle").to_string();
                let err = body["last_error"].as_str().unwrap_or("").to_string();
                if phase != last_phase {
                    println!("phase: {}", phase);
                    last_phase = phase.clone();
                }
                if !err.is_empty() && err != last_err {
                    eprintln!("update error: {}", err);
                    last_err = err.clone();
                }
                match phase.as_str() {
                    "success" => {
                        println!("Update applied. Daemon is restarting.");
                        return;
                    }
                    "error" => {
                        eprintln!(
                            "Update failed: {}",
                            if last_err.is_empty() {
                                "(no detail)"
                            } else {
                                &last_err
                            }
                        );
                        std::process::exit(1);
                    }
                    "idle" if started.elapsed() > Duration::from_secs(2) => {
                        println!("No update available.");
                        return;
                    }
                    _ => continue,
                }
            }
            Ok(_) | Err(_) => {
                connect_failures += 1;
                if connect_failures >= 6 && last_phase == "applying" {
                    println!(
                        "Daemon stopped responding while applying — assumed restart in progress."
                    );
                    return;
                }
                continue;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_continuation, resolve_local_install_continuation, ContinuationRegistration,
        DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT,
    };

    #[test]
    fn continuation_registration_reads_the_daemon_answer() {
        let registered = serde_json::json!({"tab_id": "tab-1", "registered": true});
        let refused =
            serde_json::json!({"tab_id": "tab-1", "registered": false, "reason": "opaque"});

        assert_eq!(
            classify_continuation(Some("tab-1"), &registered),
            ContinuationRegistration::Registered
        );
        assert_eq!(
            classify_continuation(Some("tab-1"), &refused),
            ContinuationRegistration::Refused("opaque".into())
        );
        assert_eq!(
            classify_continuation(Some("tab-1"), &serde_json::Value::Null),
            ContinuationRegistration::Unsupported
        );
        assert_eq!(
            classify_continuation(None, &serde_json::Value::Null),
            ContinuationRegistration::NotRequested
        );
    }

    #[test]
    fn local_install_defaults_to_resuming_the_calling_tab() {
        let (prompt, tab) =
            resolve_local_install_continuation(false, None, None, Some("tab-1".to_string()));

        assert_eq!(tab.as_deref(), Some("tab-1"));
        assert_eq!(
            prompt.as_deref(),
            Some(DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT)
        );
    }

    #[test]
    fn local_install_continuation_can_be_disabled_explicitly() {
        let (prompt, tab) = resolve_local_install_continuation(
            true,
            None,
            Some("tab-1".to_string()),
            Some("tab-2".to_string()),
        );

        assert!(prompt.is_none());
        assert!(tab.is_none());
    }
}

pub fn handle_update(
    json: bool,
    action: Option<UpdateAction>,
    manifest_url: Option<String>,
    after_update: Option<String>,
) {
    let (base_url, _) = read_api_config();
    let http = match build_local_api() {
        Ok(c) => c,
        Err(e) => {
            crate::cmd::print::fail(1, format!("failed to build HTTP client: {}", e));
        }
    };

    let resolved = action.map(|action| match action {
        UpdateAction::Install {
            auto,
            manifest_url: action_manifest_url,
            from_file,
            continue_prompt,
            continue_tab,
            no_continue,
            after_update: action_after_update,
        } => UpdateAction::Install {
            auto,
            manifest_url: action_manifest_url.or(manifest_url),
            from_file,
            continue_prompt,
            continue_tab,
            no_continue,
            after_update: action_after_update.or(after_update),
        },
        other => other,
    });

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
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&body).unwrap_or_default()
                        );
                    } else {
                        let current = body["current_version"].as_str().unwrap_or("?");
                        let latest = body["latest_version"].as_str().unwrap_or("?");
                        let method = body["install_method"].as_str().unwrap_or("unknown");
                        let available = body["update_available"].as_bool().unwrap_or(false);

                        println!(
                            "Current: v{}  Latest: v{}  Install: {}",
                            current, latest, method
                        );
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
                    crate::cmd::print::fail(1, resp.status().to_string());
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        UpdateAction::Install {
            auto,
            manifest_url,
            from_file,
            continue_prompt,
            continue_tab,
            no_continue,
            after_update,
        } => {
            if let Some(path) = from_file {
                let (continue_prompt, tab) = resolve_local_install_continuation(
                    no_continue,
                    continue_prompt,
                    continue_tab,
                    std::env::var("CODETERM_TAB_ID").ok(),
                );
                save_session_with_continuation(
                    &http,
                    &base_url,
                    continue_prompt.as_deref(),
                    tab.as_deref(),
                );
                crate::cmd::install::run_install_from_file(&path, json);
                return;
            }
            let mut params: Vec<(&str, String)> = Vec::new();
            if auto {
                params.push(("method", "auto".into()));
            }
            if let Some(m) = manifest_url {
                params.push(("manifest_url", m));
            }
            if let Some(t) = &after_update {
                params.push(("after_update", t.clone()));
            }
            // Identify the calling pane so the daemon excludes it from the
            // shutdown sweep and it survives to relaunch (Defect 3).
            let initiating_tab = if no_continue {
                None
            } else {
                continue_tab.or_else(|| std::env::var("CODETERM_TAB_ID").ok())
            };
            if let Some(tab) = &initiating_tab {
                params.push(("continue_tab", tab.clone()));
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
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&body).unwrap_or_default()
                        );
                        return;
                    }
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
                            poll_update_status(&http, &base_url);
                        }
                        "already_in_flight" => {
                            println!(
                                "{}",
                                body["message"]
                                    .as_str()
                                    .unwrap_or("An update is already being applied.")
                            );
                        }
                        _ => println!("{}", body),
                    }
                }
                Ok(resp) => {
                    crate::cmd::print::fail(1, resp.status().to_string());
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
    }
}
