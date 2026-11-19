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

fn update_failure_message(raw: &str) -> String {
    let raw = raw.split("\n[update-blocking-tabs]").next().unwrap_or(raw);
    codeterm_types::native_update::apply::NativeUpdateAdmissionRefusal::from_error(raw)
        .map(|reason| reason.message())
        .unwrap_or(raw)
        .to_string()
}

fn poll_update_status(http: &crate::local_api::LocalApiClient, base_url: &str) {
    let mut last_phase = String::new();
    loop {
        let status_url = format!(
            "{}/api/update/wait?after_phase={}",
            base_url,
            urlencoding::encode(&last_phase)
        );
        match http.get(&status_url).send() {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().unwrap_or_default();
                let phase = body["phase"].as_str().unwrap_or("idle").to_string();
                let err = update_failure_message(body["last_error"].as_str().unwrap_or(""));
                if phase != last_phase {
                    println!("phase: {}", phase);
                    last_phase = phase.clone();
                }
                match phase.as_str() {
                    "success" => {
                        if verified_update_version(&body).is_some() {
                            println!("Update verified. Version {} is running.", body["current_version"].as_str().unwrap_or("unknown"));
                            return;
                        }
                        crate::cmd::print::fail(2, "Update completion is unverified: the daemon has no matching replacement-version receipt. Run domios update status after restart.");
                    }
                    "error" => {
                        crate::cmd::print::fail(1, format!("Update failed: {}", if err.is_empty() { "(no detail)" } else { &err }));
                    }
                    "idle" => {
                        if body["native_update"]["phase"]["phase"] == "no_update" {
                            println!("No update available.");
                            return;
                        }
                        crate::cmd::print::fail(2, "Update completion is unverified: the updater became idle without a completion receipt. Run domios update status.");
                    }
                    _ => continue,
                }
            }
            Ok(resp) => crate::cmd::print::fail(2, format!("Update completion is unverified: {}. Run domios update status after restart; inspect native-update-handoff.json and native-update-installer.json in the instance data directory.", resp.text().unwrap_or_default())),
            Err(error) => crate::cmd::print::fail(2, format!("Update connection ended before the new version was verified: {error}. Run domios update status after restart; inspect native-update-handoff.json and native-update-installer.json in the instance data directory.")),
        }
    }
}

fn verified_update_version(body: &serde_json::Value) -> Option<&str> {
    let phase = &body["native_update"]["phase"];
    let version = phase["payload"]["version"].as_str()?;
    (phase["phase"] == "verified" && body["current_version"].as_str() == Some(version))
        .then_some(version)
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

fn strictly_newer(installed: &str, latest: &str) -> bool {
    use codeterm_plugins::{AvailabilityItem, AvailabilityState, UpdateKind};
    AvailabilityItem::release("", UpdateKind::App, installed, latest, true).state
        == AvailabilityState::UpdateAvailable
}

/// Plain words for what an item's state establishes; a state that vouches for nothing has none.
fn check_item_state(state: &str, reason: &str) -> Option<&'static str> {
    match (state, reason) {
        ("core_silent", _) => Some("updates automatically"),
        ("incompatible", _) | (_, "incompatible") => Some("no compatible update"),
        ("up_to_date", "same_version" | "local_ahead") => Some("up to date"),
        ("up_to_date", "local_install") => Some("installed locally"),
        _ => None,
    }
}

fn check_item_line(item: &serde_json::Value) -> String {
    let id = item["id"].as_str().unwrap_or("?");
    let Some(installed) = item["installed"].as_str() else {
        return id.to_string();
    };
    let state = item["state"].as_str().unwrap_or_default();
    let offer = match state {
        "update_available" => Some("update available"),
        "ready_to_apply" => Some("ready to apply"),
        _ => None,
    };
    if let (Some(offer), Some(latest)) = (offer, item["latest"].as_str()) {
        if strictly_newer(installed, latest) {
            return format!("{id}: {installed} → {latest} ({offer})");
        }
    }
    match check_item_state(state, item["reason"].as_str().unwrap_or_default()) {
        Some(words) => format!("{id}: {installed} ({words})"),
        None => format!("{id}: {installed}"),
    }
}

fn check_lines(body: &serde_json::Value, verbose: bool) -> Vec<String> {
    let mut lines: Vec<String> = body["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(check_item_line)
        .collect();
    if verbose {
        for (source, path) in body["served_by"].as_object().into_iter().flatten() {
            let path = match path.as_str() {
                Some("direct") => "direct download",
                Some("github_api") => "the GitHub Releases API",
                _ => "another path",
            };
            lines.push(format!("{source}: served by {path}"));
        }
        for (source, error) in body["errors"].as_object().into_iter().flatten() {
            let error = error.as_str().unwrap_or("no detail");
            lines.push(format!("{source}: check failed: {error}"));
        }
    }
    lines
}

pub fn handle_update(
    json: bool,
    verbose: bool,
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
                        println!("{}", crate::cmd::output::encode(&body).unwrap_or_default());
                    } else {
                        for line in check_lines(&body, verbose) {
                            println!("{line}");
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
                    crate::nonempty_env("CODETERM_TAB_ID"),
                );
                if no_continue {
                    if let Err(error) = save_session_with_suppression(
                        &http,
                        &base_url,
                        crate::nonempty_env("CODETERM_TAB_ID").as_deref(),
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
            let caller_tab = crate::nonempty_env("CODETERM_TAB_ID");
            if let Err(error) =
                validate_auto_continue_tab(auto, continue_tab.as_deref(), caller_tab.as_deref())
            {
                crate::cmd::print::fail(1, error);
            }
            if no_continue {
                if let Err(error) = save_session_with_suppression(
                    &http,
                    &base_url,
                    crate::nonempty_env("CODETERM_TAB_ID").as_deref(),
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
            let continuation_tab = if no_continue {
                None
            } else {
                continue_tab.or(caller_tab)
            };
            if let Some(tab) = continuation_tab {
                params.push(("continue_tab", tab));
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
                        println!("{}", crate::cmd::output::encode(&body).unwrap_or_default());
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
