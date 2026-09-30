use crate::args::sessions::BackupsAction;
use crate::SessionsAction;

#[derive(Debug, PartialEq, Eq)]
pub enum BackendOutcome {
    Success,
    Failure(String),
}

/// HTTP status is the ONLY failure signal: a 2xx stays optimistic success even
/// when the body carries no evidence that the resumed agent actually started.
pub fn classify_backend_response(
    status: reqwest::StatusCode,
    body: &serde_json::Value,
) -> BackendOutcome {
    if status.is_success() {
        BackendOutcome::Success
    } else {
        BackendOutcome::Failure(format!("{} — {}", status, body))
    }
}

/// Classification happens before any `--json` branch, so both modes exit 1 on
/// the same responses; only the rendering differs.
fn fail(json_mode: bool, human: &str, machine: &str) -> ! {
    if json_mode {
        crate::cmd::plan_output::print_plan_error(true, machine);
    } else {
        eprintln!("{human}");
    }
    std::process::exit(1);
}

fn fail_transport(json_mode: bool, error: &crate::local_api::LocalApiError) -> ! {
    let machine = error.to_string();
    fail(json_mode, &format!("Error: {machine}"), &machine)
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/_.-~".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

// UTC date helper: unix-ts -> "YYYY-MM-DD HH:MM", no chrono dependency in CLI.
fn format_unix_ts(ts: i64) -> String {
    if ts <= 0 {
        return "?".into();
    }
    let days = ts / 86_400;
    let sec_of_day = ts.rem_euclid(86_400);
    let hh = sec_of_day / 3600;
    let mm = (sec_of_day % 3600) / 60;
    // Civil-from-days algorithm (Howard Hinnant), days since 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm)
}

fn search_warning_lines(val: &serde_json::Value) -> Vec<String> {
    val.get("warnings")
        .and_then(|v| v.as_array())
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(|w| w.get("message").and_then(|m| m.as_str()))
                .map(|message| format!("warning: {message}"))
                .collect()
        })
        .unwrap_or_default()
}

/// Human rendering of a search result; warnings name every provider store the
/// search could not fully read, so an empty result is never a silent miss.
fn render_search_human(query: &str, val: &serde_json::Value) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let hits = val
        .get("hits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if hits.is_empty() {
        let _ = writeln!(out, "No sessions matched: {query}");
    } else {
        let _ = writeln!(out, "Found {} session(s) for: {query}\n", hits.len());
    }
    for (i, hit) in hits.iter().enumerate() {
        let meta = hit.get("meta").cloned().unwrap_or_default();
        let field = |key: &str| meta.get(key).and_then(|v| v.as_str()).unwrap_or("");
        let provider_id = meta.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
        let sid = meta
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let project = field("project_path");
        let title = field("title");
        let modified = meta
            .get("modified_ts")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let modified_str = format_unix_ts(modified);
        let _ = writeln!(out, "{:─<70}", "");
        let header = if title.is_empty() {
            format!("{}. [{}] [{}] {}", i + 1, provider_id, modified_str, sid)
        } else {
            format!(
                "{}. [{}] [{}] {} — {}",
                i + 1,
                provider_id,
                modified_str,
                title,
                sid
            )
        };
        let _ = writeln!(out, "  {header}");
        if !project.is_empty() {
            let _ = writeln!(out, "  project: {project}");
        }
        let _ = writeln!(
            out,
            "  resume:  codeterm sessions resume {}\n",
            shell_quote(sid)
        );
        if let Some(snips) = hit.get("snippets").and_then(|v| v.as_array()) {
            for s in snips {
                let role = s.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                let text = s.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let tag = match role {
                    "user" => "You",
                    "assistant" => provider_id,
                    other => other,
                };
                let _ = writeln!(out, "  [{tag}] {text}\n");
            }
        }
    }
    for line in search_warning_lines(val) {
        let _ = writeln!(out, "{line}");
    }
    out
}

pub fn handle_sessions(
    json_mode: bool,
    action: SessionsAction,
    http: &crate::local_api::LocalApiClient,
    base: &str,
    token: Option<&str>,
) {
    match action {
        SessionsAction::Search {
            query,
            provider,
            project,
            limit,
            max_snippets,
        } => {
            let mut url = format!(
                "{}/api/sessions/search?q={}",
                base,
                urlencoding::encode(&query)
            );
            if let Some(p) = provider {
                url.push_str(&format!("&provider={}", urlencoding::encode(&p)));
            }
            if let Some(p) = project {
                url.push_str(&format!("&project={}", urlencoding::encode(&p)));
            }
            url.push_str(&format!("&limit={}", limit));
            url.push_str(&format!("&max_snippets={}", max_snippets));

            let mut req = http.get(&url);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => fail_transport(json_mode, &e),
            };
            let status = resp.status();
            // A refused query (unknown provider) answers in plain text; keep it for the error line.
            let body = resp.text().unwrap_or_default();
            let val: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body));
            if let BackendOutcome::Failure(machine) = classify_backend_response(status, &val) {
                fail(json_mode, &format!("Error: {machine}"), &machine);
            }
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                return;
            }
            print!("{}", render_search_human(&query, &val));
        }
        SessionsAction::Resume {
            session_id,
            title,
            workspace_id,
            workspace_name,
            model,
        } => {
            let mut body = serde_json::Map::new();
            if let Some(t) = title {
                body.insert("title".into(), serde_json::Value::String(t));
            }
            if let Some(w) = workspace_id {
                body.insert("workspace_id".into(), serde_json::Value::String(w));
            }
            if let Some(n) = workspace_name {
                body.insert("workspace_name".into(), serde_json::Value::String(n));
            }
            if let Some(m) = model {
                body.insert("model".into(), serde_json::Value::String(m));
            }

            let url = format!(
                "{}/api/sessions/{}/resume",
                base,
                urlencoding::encode(&session_id)
            );
            let mut req = http.post(&url).json(&serde_json::Value::Object(body));
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            match req.send() {
                Ok(resp) => {
                    let status = resp.status();
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if let BackendOutcome::Failure(machine) =
                        classify_backend_response(status, &val)
                    {
                        fail(json_mode, &format!("Error: {machine}"), &machine);
                    }
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                        return;
                    }
                    let tab = val.get("tab_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let ws = val
                        .get("workspace_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let provider = val.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                    let cwd = val.get("cwd").and_then(|v| v.as_str()).unwrap_or("?");
                    let identity = val.get("identity").cloned().unwrap_or_default();
                    let identity_status = identity
                        .get("status")
                        .and_then(|value| value.as_str())
                        .unwrap_or("roleless");
                    if identity_status == "restored" {
                        println!("Resumed [{}] {}", provider, session_id);
                        let role = identity
                            .get("role")
                            .and_then(|value| value.as_str())
                            .unwrap_or("roleless");
                        println!("  identity:  restored {role}");
                    } else {
                        println!("resumed roleless [{}] {}", provider, session_id);
                        if let Some(actions) = identity.get("actions") {
                            if let Some(promotion) =
                                actions.get("promotion").and_then(|value| value.as_str())
                            {
                                println!("  promote:   {promotion}");
                            }
                            if let Some(relink) =
                                actions.get("relink").and_then(|value| value.as_str())
                            {
                                println!("  relink:    {relink}");
                            }
                        }
                    }
                    println!("  workspace: {}", ws);
                    println!("  tab:       {}", tab);
                    println!("  cwd:       {}", cwd);
                }
                Err(e) => fail_transport(json_mode, &e),
            }
        }
        SessionsAction::Backups { action } => {
            handle_backups(json_mode, action, http, base, token);
        }
        SessionsAction::Get { session_id } => {
            let url = format!("{}/api/sessions/{}", base, urlencoding::encode(&session_id));
            let mut req = http.get(&url);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            match req.send() {
                Ok(resp) => {
                    let status = resp.status();
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if let BackendOutcome::Failure(machine) =
                        classify_backend_response(status, &val)
                    {
                        fail(json_mode, &format!("Error: {machine}"), &machine);
                    }
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                        return;
                    }
                    let provider = val.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                    let cwd = val
                        .get("project_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let file = val.get("file_path").and_then(|v| v.as_str()).unwrap_or("-");
                    let modified = val.get("modified_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                    println!("session: {}", session_id);
                    println!("  provider: {}", provider);
                    println!("  cwd:      {}", cwd);
                    println!("  file:     {}", file);
                    println!("  modified: {}", format_unix_ts(modified));
                }
                Err(e) => fail_transport(json_mode, &e),
            }
        }
    }
}

fn handle_backups(
    json_mode: bool,
    action: BackupsAction,
    http: &crate::local_api::LocalApiClient,
    base: &str,
    token: Option<&str>,
) {
    match action {
        BackupsAction::List { limit } => {
            let url = format!("{}/api/session/backups", base);
            let mut req = http.get(&url);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => fail_transport(json_mode, &e),
            };
            let status = resp.status();
            let val: serde_json::Value = resp.json().unwrap_or_default();
            if let BackendOutcome::Failure(machine) = classify_backend_response(status, &val) {
                fail(json_mode, &format!("Error: {machine}"), &machine);
            }
            let backups: Vec<serde_json::Value> = val
                .get("backups")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(limit)
                .collect();
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "backups": backups }))
                        .unwrap_or_default()
                );
                return;
            }
            if backups.is_empty() {
                println!("No session backups on disk yet.");
                return;
            }
            println!("{} session backup(s), newest first:\n", backups.len());
            println!(
                "{:<14} {:<18} {:>10} {:>6}",
                "NAME", "SAVED (UTC)", "WORKSPACES", "TABS"
            );
            for b in &backups {
                let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let saved = b.get("saved_at_secs").and_then(|v| v.as_i64()).unwrap_or(0);
                let ws = b.get("workspaces").and_then(|v| v.as_u64()).unwrap_or(0);
                let tabs = b.get("tabs").and_then(|v| v.as_u64()).unwrap_or(0);
                println!(
                    "{:<14} {:<18} {:>10} {:>6}",
                    name,
                    format_unix_ts(saved),
                    ws,
                    tabs
                );
            }
            println!("\nRestore one (additive, replaces nothing): codeterm sessions backups restore <NAME>");
        }
        BackupsAction::Restore { name } => {
            let url = format!(
                "{}/api/session/backups/{}/restore",
                base,
                urlencoding::encode(&name)
            );
            let mut req = http.post(&url);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => fail_transport(json_mode, &e),
            };
            let status = resp.status();
            let val: serde_json::Value = resp.json().unwrap_or_default();
            if let BackendOutcome::Failure(machine) = classify_backend_response(status, &val) {
                fail(json_mode, &format!("Error: {machine}"), &machine);
            }
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                return;
            }
            let ws = val
                .get("workspaces_added")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let tabs = val.get("tabs_added").and_then(|v| v.as_u64()).unwrap_or(0);
            let present = val
                .get("tabs_already_present")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("Restored backup {} into the live desk (additive):", name);
            println!("  workspaces recreated: {}", ws);
            println!("  tabs recreated:       {}", tabs);
            println!("  already present:      {} (left untouched)", present);
            if ws == 0 && tabs == 0 {
                println!("Nothing was missing — the live desk already covers this backup.");
            }
        }
    }
}

#[cfg(test)]
mod tests;
