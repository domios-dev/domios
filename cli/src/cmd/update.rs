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

fn validate_auto_continue_tab(
    auto: bool,
    requested_tab: Option<&str>,
    caller_tab: Option<&str>,
) -> Result<(), &'static str> {
    if auto && requested_tab.is_some_and(|requested| caller_tab != Some(requested)) {
        Err("--continue-tab must match the authenticated caller tab")
    } else {
        Ok(())
    }
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

fn session_save_action(
    prompt: Option<&str>,
    tab: Option<&str>,
    suppress: bool,
) -> Option<serde_json::Value> {
    let tab = tab?;
    if suppress {
        Some(serde_json::json!({ "suppress_continue_tab": tab }))
    } else {
        Some(serde_json::json!({
            "continue_tab": tab,
            "continue_prompt": prompt,
        }))
    }
}

fn post_session_save(
    http: &crate::local_api::LocalApiClient,
    base_url: &str,
    action: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let save_url = format!("{}/api/session/save", base_url);
    let request = http.post(&save_url);
    let request = match action {
        Some(body) => request.json(&body),
        None => request,
    };
    request
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.json::<serde_json::Value>())
        .map_err(|error| error.to_string())
}

fn continuation_withdraw_action(
    prompt: Option<&str>,
    tab: Option<&str>,
) -> Option<serde_json::Value> {
    let tab = tab?;
    Some(serde_json::json!({
        "withdraw_continue_tab": tab,
        "withdraw_continue_prompt": prompt,
    }))
}

/// Persist the desk and hand the calling tab's continuation to the daemon that
/// restarts; the next start delivers it once that tab's session is agent-ready.
fn save_session_with_continuation(
    http: &crate::local_api::LocalApiClient,
    base_url: &str,
    prompt: Option<&str>,
    tab: Option<&str>,
) -> ContinuationRegistration {
    let saved = match post_session_save(http, base_url, session_save_action(prompt, tab, false)) {
        Ok(saved) => saved,
        Err(e) => {
            eprintln!("[install --from-file] WARN: session save failed ({e}) — Tab id may rotate");
            return if tab.is_some() {
                ContinuationRegistration::Unsupported
            } else {
                ContinuationRegistration::NotRequested
            };
        }
    };
    let registration = classify_continuation(tab, &saved["continuation"]);
    match &registration {
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
    registration
}

fn save_session_with_suppression(
    http: &crate::local_api::LocalApiClient,
    base_url: &str,
    tab: Option<&str>,
) -> Result<(), String> {
    let Some(tab) = tab else {
        return Ok(());
    };
    let saved = post_session_save(http, base_url, session_save_action(None, Some(tab), true))?;
    suppression_result(&saved)
}

fn suppression_result(saved: &serde_json::Value) -> Result<(), String> {
    if saved["suppression"]["suppressed"] == true {
        return Ok(());
    }
    if let Some(reason) = saved["suppression"]["reason"].as_str() {
        return Err(format!("caller continuation was not suppressed: {reason}"));
    }
    if saved["suppression"]["suppressed"] == false {
        return Ok(());
    }
    Err("daemon did not confirm caller continuation suppression".into())
}

fn withdraw_session_continuation(
    http: &crate::local_api::LocalApiClient,
    base_url: &str,
    prompt: Option<&str>,
    tab: Option<&str>,
) {
    let Some(action) = continuation_withdraw_action(prompt, tab) else {
        return;
    };
    if let Err(error) = post_session_save(http, base_url, Some(action)) {
        eprintln!("[install] WARN: failed to withdraw continuation after install failure: {error}");
    }
}

fn install_with_withdrawal(
    install: impl FnOnce() -> Result<(), String>,
    withdraw: impl FnOnce(),
) -> Result<(), String> {
    match install() {
        Ok(()) => Ok(()),
        Err(error) => {
            withdraw();
            Err(error)
        }
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
            eprintln!("Timed out after {}s waiting for update to complete. Check ~/.domios/logs/domios.log for details.", timeout.as_secs());
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
mod tests;

fn container_update_error(marked: bool) -> Option<crate::cmd::print::CliError> {
    marked.then(|| crate::cmd::print::CliError {
        message: codeterm_paths::container_install::UPDATE_MESSAGE.into(),
        code: None,
        data: Some(serde_json::json!({
            "code": codeterm_paths::container_install::UPDATE_CODE,
        })),
    })
}

pub fn handle_update(
    json: bool,
    action: Option<UpdateAction>,
    manifest_url: Option<String>,
    after_update: Option<String>,
) {
    if let Some(error) = container_update_error(codeterm_paths::container_install::marked()) {
        crate::cmd::print::fail_with(1, &error);
    }
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
                format!("{}/api/update/availability?force=true", base_url)
            } else {
                format!("{}/api/update/availability", base_url)
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
                        for item in body["items"].as_array().into_iter().flatten() {
                            println!(
                                "{}: {} → {} ({})",
                                item["id"].as_str().unwrap_or("?"),
                                item["installed"].as_str().unwrap_or("unknown"),
                                item["latest"].as_str().unwrap_or("unknown"),
                                item["state"].as_str().unwrap_or("unknown")
                            );
                        }
                        for (source, error) in body["errors"].as_object().into_iter().flatten() {
                            println!(
                                "WARNING: {source}: {}",
                                error.as_str().unwrap_or("discovery failed")
                            );
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.text().unwrap_or_default();
                    crate::cmd::print::fail_http(1, status, &body);
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
                if no_continue {
                    if let Err(error) = save_session_with_suppression(
                        &http,
                        &base_url,
                        std::env::var("CODETERM_TAB_ID").ok().as_deref(),
                    ) {
                        crate::cmd::print::fail(
                            1,
                            format!("--no-continue could not be honored: {error}"),
                        );
                    }
                }
                if !no_continue {
                    let _ = save_session_with_continuation(
                        &http,
                        &base_url,
                        continue_prompt.as_deref(),
                        tab.as_deref(),
                    );
                }
                if let Err(error) = install_with_withdrawal(
                    || crate::cmd::install::run_install_from_file(&path, json),
                    || {
                        withdraw_session_continuation(
                            &http,
                            &base_url,
                            continue_prompt.as_deref(),
                            tab.as_deref(),
                        )
                    },
                ) {
                    crate::cmd::print::fail(1, error);
                }
                return;
            }
            let mut params: Vec<(&str, String)> = Vec::new();
            if auto {
                params.push(("method", "auto".into()));
            }
            let caller_tab = std::env::var("CODETERM_TAB_ID").ok();
            if let Err(error) =
                validate_auto_continue_tab(auto, continue_tab.as_deref(), caller_tab.as_deref())
            {
                crate::cmd::print::fail(1, error);
            }
            if no_continue {
                if let Err(error) = save_session_with_suppression(
                    &http,
                    &base_url,
                    std::env::var("CODETERM_TAB_ID").ok().as_deref(),
                ) {
                    crate::cmd::print::fail(
                        1,
                        format!("--no-continue could not be honored: {error}"),
                    );
                }
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
                continue_tab.or(caller_tab)
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
                    let status = resp.status().as_u16();
                    let body = resp.text().unwrap_or_default();
                    crate::cmd::print::fail_http(1, status, &body);
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
    }
}
