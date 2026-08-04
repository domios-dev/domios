use crate::SessionsAction;

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
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let val: serde_json::Value = resp.json().unwrap_or_default();
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                return;
            }
            let hits = val
                .get("hits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if hits.is_empty() {
                println!("No sessions matched: {}", query);
                return;
            }
            println!("Found {} session(s) for: {}\n", hits.len(), query);
            for (i, hit) in hits.iter().enumerate() {
                let meta = hit.get("meta").cloned().unwrap_or_default();
                let provider_id = meta.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                let sid = meta
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let project = meta
                    .get("project_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let title = meta.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let modified = meta
                    .get("modified_ts")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let modified_str = format_unix_ts(modified);
                println!("{:─<70}", "");
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
                println!("  {}", header);
                if !project.is_empty() {
                    println!("  project: {}", project);
                }
                // Claude resolves --resume by encoded cwd, so include a `cd <project_path>` hint when known.
                let resume_cmd = match provider_id {
                    "claude" => format!("claude --resume {}", sid),
                    "codex" => format!("codex resume {}", sid),
                    other => format!("{} resume {}", other, sid),
                };
                let resume_hint = if !project.is_empty() {
                    format!("cd {} && {}", shell_quote(project), resume_cmd)
                } else {
                    resume_cmd
                };
                println!("  resume:  {}", resume_hint);
                println!();
                if let Some(snips) = hit.get("snippets").and_then(|v| v.as_array()) {
                    for s in snips {
                        let role = s.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                        let text = s.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let tag = match role {
                            "user" => "You".to_string(),
                            "assistant" => match provider_id {
                                "claude" => "Claude".to_string(),
                                "codex" => "Codex".to_string(),
                                other => other.to_string(),
                            },
                            other => other.to_string(),
                        };
                        println!("  [{}] {}", tag, text);
                        println!();
                    }
                }
            }
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
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                        return;
                    }
                    if !status.is_success() {
                        eprintln!("Error: {} — {}", status, val);
                        std::process::exit(1);
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
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
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
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                        return;
                    }
                    if !status.is_success() {
                        eprintln!("Error: {} — {}", status, val);
                        std::process::exit(1);
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
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
