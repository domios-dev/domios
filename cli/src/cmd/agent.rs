use crate::args::agent::ReportArgs;
use crate::client::SocketClient;
use crate::cmd::hints::maybe_orchestration_reminder;
use crate::cmd::plan_errors::render_plan_call_error;
use crate::cmd::print::{handle_result, print_object};
use crate::cmd::report_guard::ReportRefusal;
use crate::{build_http, read_api_config, resolve_pane, AgentAction};
use std::io::Read;

/// Resolve the spawn task body from `--task` / `--task-file`. `--task-file`
/// reads the file verbatim (or stdin when the value is `-`), so the body never
/// passes through the caller's shell — backticks and $(...) survive byte-exact.
pub fn resolve_spawn_task<R: Read>(
    task: Option<String>,
    task_file: Option<String>,
    stdin: &mut R,
) -> Result<Option<String>, String> {
    match (task, task_file) {
        (Some(_), Some(_)) => Err("pass either --task or --task-file, not both".into()),
        (Some(t), None) => Ok(Some(t)),
        (None, Some(path)) => {
            if path == "-" {
                let mut buf = String::new();
                stdin
                    .read_to_string(&mut buf)
                    .map_err(|e| format!("failed to read --task-file from stdin: {}", e))?;
                Ok(Some(buf))
            } else {
                std::fs::read_to_string(&path)
                    .map(Some)
                    .map_err(|e| format!("failed to read --task-file {}: {}", path, e))
            }
        }
        (None, None) => Ok(None),
    }
}

pub fn resolve_spawn_charter<R: Read>(
    charter: Option<String>,
    charter_file: Option<String>,
    stdin: &mut R,
) -> Result<Option<String>, String> {
    match (charter, charter_file) {
        (Some(_), Some(_)) => Err("pass either --charter or --charter-file, not both".into()),
        (Some(t), None) => Ok(Some(t)),
        (None, Some(path)) => {
            if path == "-" {
                let mut buf = String::new();
                stdin
                    .read_to_string(&mut buf)
                    .map_err(|e| format!("failed to read --charter-file from stdin: {}", e))?;
                Ok(Some(buf))
            } else {
                std::fs::read_to_string(&path)
                    .map(Some)
                    .map_err(|e| format!("failed to read --charter-file {}: {}", path, e))
            }
        }
        (None, None) => Ok(None),
    }
}

pub(crate) fn parse_duration_secs(value: &str, flag: &str) -> Result<u64, String> {
    let s = value.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(format!("{flag} is empty"));
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s
            .parse::<u64>()
            .map(|minutes| minutes.saturating_mul(60))
            .map_err(|e| e.to_string());
    }
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut i = 0usize;
    let mut total = 0u64;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            return Err(format!("invalid {flag} value: {value}"));
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let amount: u64 = chars[start..i]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        if i >= chars.len() {
            return Err(format!("missing unit in {flag} value: {value}"));
        }
        let unit = chars[i];
        i += 1;
        let factor = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            _ => return Err(format!("unsupported {flag} unit: {unit}")),
        };
        total = total.saturating_add(amount.saturating_mul(factor));
    }
    Ok(total)
}

pub(crate) const MAX_PANE_DESCRIPTION_CHARS: usize = 200;

pub(crate) fn join_cli_words(value: Option<Vec<String>>) -> Option<String> {
    value
        .map(|words| words.join(" "))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn refusal_next_commands() -> Vec<&'static str> {
    vec![
        "git status --short",
        "git add <files>",
        "git commit -m \"<message>\"",
        "codeterm agent report --status in_review --sha $(git rev-parse HEAD) \"...\"",
        "codeterm agent report --status partial \"...\"",
    ]
}

fn refusal_report_message(refusal: &ReportRefusal, original: &str) -> String {
    format!(
        "REPORT NOT SENT\n{}\n\nOriginal readiness report that was refused locally:\n{}",
        refusal.message(),
        original
    )
}

fn format_report_refusal_text(
    refusal: &ReportRefusal,
    sent: bool,
    report_id: Option<&str>,
    error: Option<&str>,
) -> String {
    let mut lines = vec!["REPORT NOT SENT".to_string(), refusal.message()];
    match (sent, report_id, error) {
        (true, Some(id), _) => lines.push(format!("Diagnostic report sent to parent: {id}")),
        (true, None, _) => lines.push("Diagnostic report sent to parent.".to_string()),
        (false, _, Some(err)) => lines.push(format!("Diagnostic report could not be sent: {err}")),
        (false, _, None) => lines.push("Diagnostic report could not be sent.".to_string()),
    }
    lines.push("REPORT NOT SENT".to_string());
    format!("{}\n", lines.join("\n"))
}

fn handle_report_refusal(
    client: &mut SocketClient,
    json: bool,
    mut body: serde_json::Value,
    refusal: ReportRefusal,
) -> ! {
    let original = body
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    body["message"] = serde_json::json!(refusal_report_message(&refusal, &original));
    body["kind"] = serde_json::json!("report_refused");
    body["status"] = serde_json::json!("partial");

    let diagnostic = client.call_detailed("agent.report", Some(body.clone()));
    let (sent, report_id, error) = match diagnostic {
        Ok(value) => (
            true,
            value
                .get("report_id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            None,
        ),
        Err(err) => {
            let rendered = if body.get("plan_id").and_then(|v| v.as_str()).is_some() {
                render_plan_call_error(err, &body)
            } else {
                err.to_string()
            };
            (false, None, Some(rendered))
        }
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "verdict": "REPORT NOT SENT",
                "reason": refusal.reason,
                "dirty_files": refusal.dirty_files,
                "next_commands": refusal_next_commands(),
                "diagnostic_report_sent": sent,
                "diagnostic_report_id": report_id,
                "diagnostic_error": error,
            }))
            .unwrap()
        );
    } else {
        eprint!(
            "{}",
            format_report_refusal_text(&refusal, sent, report_id.as_deref(), error.as_deref())
        );
    }
    std::process::exit(1);
}

pub(crate) fn normalize_pane_description(value: &str) -> Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.chars().count() > MAX_PANE_DESCRIPTION_CHARS {
        return Err(format!(
            "description must be {} characters or fewer",
            MAX_PANE_DESCRIPTION_CHARS
        ));
    }
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn env_default(current: Option<String>, var: &str) -> Option<String> {
    current.or_else(|| {
        std::env::var(var)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

fn print_report_notices(json: bool, val: &serde_json::Value) {
    if json {
        return;
    }
    if let Some(arr) = val.get("notices").and_then(|v| v.as_array()) {
        for notice in arr {
            if let Some(s) = notice.as_str() {
                eprintln!("notice: {}", s);
            }
        }
    }
}

fn format_agent_models(target: &serde_json::Value, provider: &str) -> String {
    let models = target.get("available_models").and_then(|v| v.as_array());
    let Some(models) = models else {
        return format!("No models advertised for provider '{}'.\n", provider);
    };
    let default_model = target.get("default_model").and_then(|v| v.as_str());
    let mut out = String::new();
    for m in models {
        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
        let marker = if default_model == Some(id) {
            " (default)"
        } else {
            ""
        };
        let description = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
        if description.is_empty() {
            out.push_str(&format!("{:<10} {}{}\n", id, name, marker));
        } else {
            out.push_str(&format!(
                "{:<10} {}{}\n             {}\n",
                id, name, marker, description
            ));
        }
    }
    out.push_str(&format_reasoning_efforts(target));
    out
}

fn format_reasoning_efforts(target: &serde_json::Value) -> String {
    let efforts = target
        .get("reasoning_efforts")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty());
    let Some(efforts) = efforts else {
        return String::new();
    };
    let default = target
        .get("default_reasoning_effort")
        .and_then(|v| v.as_str());
    let mut out = String::from("\nReasoning effort:\n");
    for e in efforts {
        let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = e.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
        let marker = if default == Some(id) {
            " (default)"
        } else {
            ""
        };
        out.push_str(&format!("{:<10} {}{}\n", id, name, marker));
    }
    out
}

pub fn handle_agent(client: &mut SocketClient, json: bool, action: AgentAction) {
    let (base, token) = read_api_config();
    let http = match build_http(&token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };
    match action {
        AgentAction::Providers => match http.get(format!("{}/api/agents/providers", base)).send() {
            Ok(resp) => {
                let status = resp.status();
                let val: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    return;
                }
                let Some(arr) = val.as_array() else {
                    if status == 404 {
                        eprintln!(
                                "Server doesn't know /api/agents/providers — update the running CodeTerm instance."
                            );
                    } else {
                        eprintln!("Unexpected response shape (HTTP {})", status);
                    }
                    std::process::exit(1);
                };
                if arr.is_empty() {
                    println!("No agent providers registered.");
                    return;
                }
                for p in arr {
                    let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("agent");
                    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let name = p.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                    let default_model = p
                        .get("default_model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let models_count = p
                        .get("available_models")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    println!(
                        "{:<12} {:<6} {:<20} default={}  models={}",
                        id, kind, name, default_model, models_count
                    );
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        AgentAction::Models { provider } => {
            match http.get(format!("{}/api/agents/providers", base)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    let Some(arr) = val.as_array() else {
                        eprintln!("Unexpected response shape");
                        std::process::exit(1);
                    };
                    let target = arr
                        .iter()
                        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(provider.as_str()));
                    let Some(target) = target else {
                        let known: Vec<&str> = arr
                            .iter()
                            .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
                            .collect();
                        eprintln!(
                            "Unknown provider: '{}'. Known: [{}]",
                            provider,
                            known.join(", ")
                        );
                        std::process::exit(1);
                    };
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                target
                                    .get("available_models")
                                    .unwrap_or(&serde_json::Value::Null)
                            )
                            .unwrap()
                        );
                        return;
                    }
                    print!("{}", format_agent_models(target, &provider));
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::SetOrchestrator { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http
                .post(format!("{}/api/panes/{}/role", base, pane_id))
                .json(&serde_json::json!({"role": "Orchestrator"}))
                .send()
            {
                Ok(resp) => print_agent_mutation_response(resp),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Link { parent, pane } => {
            let pane_id = resolve_pane(client, pane);
            let parent_resp = http
                .get(format!("{}/api/panes/{}/status", base, parent))
                .send();
            let role = match parent_resp {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    match val.get("role").and_then(|v| v.as_str()) {
                        Some("Orchestrator") | Some("Conductor") => "Worker",
                        _ => {
                            eprintln!("Parent must be an Orchestrator");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            match http
                .post(format!("{}/api/panes/{}/role", base, pane_id))
                .json(&serde_json::json!({"role": role, "parent_pane_id": parent}))
                .send()
            {
                Ok(resp) => print_agent_mutation_response(resp),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Unlink { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http
                .delete(format!("{}/api/panes/{}/role", base, pane_id))
                .send()
            {
                Ok(resp) => print_agent_mutation_response(resp),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::AllowSpawn { pane, enable } => {
            let pane_id = resolve_pane(client, pane);
            match http
                .post(format!("{}/api/panes/{}/can-spawn", base, pane_id))
                .json(&serde_json::json!({"can_spawn": enable}))
                .send()
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp.json().unwrap_or_default();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Report(ReportArgs {
            message,
            text,
            kind,
            doc,
            pane,
            plan,
            task,
            status,
            branch,
            sha,
            base,
            changed_files,
            test_cmd,
            test_result,
            skipped,
            acknowledge_dirty,
            attachments,
        }) => {
            if let Some(ref explicit) = pane {
                if let Ok(own) = std::env::var("CODETERM_PANE_ID") {
                    let differs = !own.is_empty()
                        && own != *explicit
                        && !own.starts_with(explicit.as_str())
                        && !explicit.starts_with(own.as_str());
                    if differs {
                        eprintln!(
                            "Error: `agent report --tab {explicit}` reports AS that tab, not TO it.\n\
                             Reports route to your parent automatically — drop --tab to report up the hierarchy.\n\
                             To send a message to a specific tab, use `codeterm send --tab {explicit}`."
                        );
                        std::process::exit(1);
                    }
                }
            }
            let pane_id = resolve_pane(client, pane);
            let mut message = text.or(message).unwrap_or_default();
            if let Some(path) = doc {
                let doc_text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Error: failed to read --doc {}: {}", path, e);
                        std::process::exit(1);
                    }
                };
                if !message.is_empty() {
                    message.push_str("\n\n");
                }
                message.push_str(&format!("doc: {path}\n\n{doc_text}"));
            }
            if message.is_empty() {
                eprintln!("Error: report message, --text, or --doc is required");
                std::process::exit(1);
            }
            let plan = env_default(plan, "CODETERM_PLAN_ID");
            let task = env_default(task, "CODETERM_TASK_ID");
            let mut body = serde_json::json!({
                "pane_id": pane_id,
                "message": message,
            });
            if let Some(k) = kind {
                body["kind"] = serde_json::json!(k);
            }
            if let Some(ref p) = plan {
                body["plan_id"] = serde_json::json!(p);
            }
            if let Some(ref t) = task {
                body["task_id"] = serde_json::json!(t);
            }
            if let Some(s) = status {
                body["status"] = serde_json::json!(s.as_str());
            }
            let (branch, sha, changed_files) = match crate::cmd::report_guard::enrich_ready_report(
                status,
                branch,
                sha,
                base,
                changed_files,
                acknowledge_dirty,
            ) {
                Ok(fields) => fields,
                Err(refusal) => handle_report_refusal(client, json, body, refusal),
            };
            if acknowledge_dirty {
                if let Some(ref files) = changed_files {
                    if !files.is_empty() {
                        eprintln!(
                            "[acknowledge-dirty] overriding dirty check: {}",
                            files.join(", ")
                        );
                        let milestone_text = format!("acknowledge-dirty: {}", files.join(", "));
                        if let (Some(ref pid), Some(ref tid)) = (plan, task) {
                            let ms_params = serde_json::json!({
                                "plan_id": pid,
                                "task_id": tid,
                                "kind": "qa",
                                "text": milestone_text,
                                "from": crate::cmd::plan::owner_from_env(),
                            });
                            let _ = crate::cmd::plan::call_plan_rpc(
                                client,
                                "plan.milestone.add",
                                ms_params,
                            );
                        }
                    }
                }
            }
            if let Some(b) = branch {
                body["branch"] = serde_json::json!(b);
            }
            if let Some(s) = sha {
                body["sha"] = serde_json::json!(s);
            }
            if let Some(cf) = changed_files {
                body["changed_files"] = serde_json::json!(cf);
            }
            if let Some(c) = test_cmd {
                body["test_cmd"] = serde_json::json!(c);
            }
            if let Some(r) = test_result {
                body["test_result"] = serde_json::json!(r);
            }
            if let Some(sk) = skipped {
                body["skipped"] = serde_json::json!(sk);
            }
            if !attachments.is_empty() {
                body["attachments"] = serde_json::json!(attachments);
            }
            let result = client.call_detailed("agent.report", Some(body.clone()));
            let result = result.map_err(|err| {
                let rendered = if body.get("plan_id").and_then(|v| v.as_str()).is_some() {
                    render_plan_call_error(err, &body)
                } else {
                    err.to_string()
                };
                format!(
                    "!!! REPORT NOT ACCEPTED — no task transition or delivery was recorded.\n\
                     Fix the cause and re-run the exact same `codeterm agent report` command.\n\
                     cause: {rendered}"
                )
            });
            if let Ok(ref val) = result {
                print_report_notices(json, val);
            }
            handle_result(json, result);
        }
        AgentAction::Spawn {
            agent,
            title,
            description,
            cwd,
            role,
            parent,
            plan,
            report_to,
            can_spawn,
            task,
            task_file,
            args,
            workspace,
            model,
            reasoning_effort,
            system_prompt,
            preset,
            charter,
            charter_file,
            interval,
            nudge_cooldown,
            max_actions,
            bound_task,
            plan_task,
        } => {
            if matches!(role, Some(crate::args::agent::SpawnRole::Orchestrator))
                && workspace.is_none()
                && parent.is_none()
            {
                eprintln!("Error: --workspace is required when spawning --role Orchestrator");
                std::process::exit(1);
            }
            let task = match resolve_spawn_task(task, task_file, &mut std::io::stdin()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let charter = match resolve_spawn_charter(charter, charter_file, &mut std::io::stdin())
            {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let interval = match interval
                .as_deref()
                .map(|v| parse_duration_secs(v, "--interval"))
                .transpose()
            {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let nudge_cooldown = match nudge_cooldown
                .as_deref()
                .map(|v| parse_duration_secs(v, "--nudge-cooldown"))
                .transpose()
            {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            for hint in crate::cmd::hints::worker_spawn_hints(
                matches!(role, Some(crate::args::agent::SpawnRole::Worker)),
                parent.is_some() || report_to.is_some(),
                task.is_some(),
            ) {
                eprintln!("{hint}");
            }
            let mut body = serde_json::json!({
                "agent": agent,
            });
            if let Some(t) = join_cli_words(title) {
                body["title"] = serde_json::json!(t);
            }
            if let Some(d) = join_cli_words(description) {
                match normalize_pane_description(&d) {
                    Ok(Some(d)) => body["description"] = serde_json::json!(d),
                    Ok(None) => body["description"] = serde_json::json!(""),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            if let Some(c) = cwd {
                body["cwd"] = serde_json::json!(c);
            }
            if let Some(r) = role {
                body["role"] = serde_json::json!(r.as_str());
                if let Some(profile) = r.role_profile() {
                    body["role_profile"] = serde_json::json!(profile);
                }
            }
            if let Some(p) = parent {
                body["parent_pane_id"] = serde_json::json!(p);
            }
            if let Some(pl) = plan {
                body["plan_id"] = serde_json::json!(pl);
            }
            if let Some(rt) = report_to {
                body["report_to"] = serde_json::json!(rt);
            }
            if can_spawn {
                body["can_spawn"] = serde_json::json!(true);
            }
            if let Some(t) = task {
                body["task"] = serde_json::json!(t);
            }
            if let Some(a) = args {
                body["args"] = serde_json::json!(a);
            }
            if let Some(w) = workspace {
                body["workspace_id"] = serde_json::json!(w);
            }
            if let Some(m) = model {
                body["model"] = serde_json::json!(m);
            }
            if let Some(effort) = reasoning_effort {
                body["reasoning_effort"] = serde_json::json!(effort);
            }
            if let Some(system_prompt) = system_prompt {
                body["system_prompt"] = serde_json::json!(system_prompt);
            }
            if let Some(preset) = preset {
                body["preset"] = serde_json::json!(preset);
            }
            if let Some(charter) = charter {
                body["charter"] = serde_json::json!(charter);
            }
            if let Some(interval) = interval {
                body["tick_interval_secs"] = serde_json::json!(interval);
            }
            if let Some(nudge_cooldown) = nudge_cooldown {
                body["nudge_cooldown_secs"] = serde_json::json!(nudge_cooldown);
            }
            if let Some(max_actions) = max_actions {
                body["max_actions_per_tick"] = serde_json::json!(max_actions);
            }
            if let Some(tid) = bound_task {
                body["task_id"] = serde_json::json!(tid);
            }
            if let Some(pt) = plan_task {
                body["plan_task"] = serde_json::json!(pt);
            }
            // Record the spawner's pane for report-routing fallback.
            if let Ok(spawner) = std::env::var("CODETERM_PANE_ID") {
                if !spawner.is_empty() {
                    body["spawned_by_pane_id"] = serde_json::json!(spawner);
                }
            }
            match client.call("agent.spawn", Some(body)) {
                Ok(val) => {
                    if json {
                        crate::cmd::print::print_json(&val);
                        return;
                    }
                    let pid = val.get("pane_id").and_then(|v| v.as_str());
                    let title_str = val.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    let agent_str = val.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
                    let resolved = val
                        .get("model")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|model| format!("{agent_str}/{model}"))
                        .unwrap_or_else(|| agent_str.to_string());
                    let warning = val.get("warning").and_then(|v| v.as_str());
                    if let Some(pid) = pid {
                        println!("Spawned {resolved}: {title_str} (pane {pid})");
                    } else if let Some(err) = val.get("error").or_else(|| val.get("message")) {
                        eprintln!("Error: {}", err);
                    } else {
                        print_object(&val);
                    }
                    if let Some(warning) = warning {
                        eprintln!("Warning: {}", warning);
                    }
                    if let Some(hint) = maybe_orchestration_reminder() {
                        println!("{hint}");
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Ledger { pane } => {
            let pane_id = resolve_pane(client, pane);
            match client.call(
                "agent.ledger",
                Some(serde_json::json!({ "pane_id": pane_id })),
            ) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No workers in ledger for {}.", pane_id);
                        }
                        for r in arr {
                            let worker = r
                                .get("worker_pane_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?");
                            let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
                            let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("-");
                            let branch = r.get("branch").and_then(|v| v.as_str()).unwrap_or("-");
                            let model = r.get("model").and_then(|v| v.as_str()).unwrap_or("-");
                            let seen = r.get("updated_at").and_then(|v| v.as_u64()).unwrap_or(0);
                            println!(
                                "{:<10} {:<6} {:<14} {:<10} seen={}  {}",
                                worker, status, branch, model, seen, title
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Compact { pane } => {
            let pane_id = resolve_pane(client, pane);
            let cmd = "/compact\n";
            match http
                .post(format!("{}/api/panes/{}/send", base, pane_id))
                .json(&serde_json::json!({"text": cmd}))
                .send()
            {
                Ok(_) => {
                    println!("Sent /compact to {}", pane_id);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Restart {
            pane,
            force,
            grace_secs,
        } => {
            let pane_id = resolve_pane(client, pane);
            let body = serde_json::json!({"force": force, "grace_secs": grace_secs});
            match http
                .post(format!("{}/api/panes/{}/agent/restart", base, pane_id))
                .json(&body)
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let retry = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("30");
                        eprintln!("Error: rate limited (retry after {}s)", retry);
                        std::process::exit(1);
                    }
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if !status.is_success() {
                        let msg = val
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or_else(|| val.as_str().unwrap_or("restart failed"));
                        eprintln!("Error: {}", msg);
                        std::process::exit(1);
                    }
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let action = val
                            .get("session_action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("fresh");
                        let profile = val
                            .get("permission_profile")
                            .and_then(|v| v.as_str())
                            .unwrap_or("sandboxed");
                        let sid = val.get("session_id").and_then(|v| v.as_str());
                        let session = match (action, sid) {
                            ("resume", Some(s)) => format!("resume {}", s),
                            _ => "fresh session".to_string(),
                        };
                        println!(
                            "Restarting pane {}: {}, profile={} (same pane id, same session unless stale)",
                            pane_id, session, profile
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Model { name, pane } => {
            let pane_id = resolve_pane(client, pane);
            let agent_type = http
                .get(format!("{}/api/panes", base))
                .send()
                .ok()
                .and_then(|r| r.json::<serde_json::Value>().ok())
                .and_then(|v| {
                    v.as_array().and_then(|arr| {
                        arr.iter()
                            .find(|p| p.get("id").and_then(|i| i.as_str()) == Some(&pane_id))
                            .and_then(|p| {
                                p.get("agent_type")
                                    .and_then(|t| t.as_str())
                                    .map(String::from)
                            })
                    })
                });
            if matches!(agent_type.as_deref(), Some("codex")) {
                eprintln!("Error: Codex /model is an interactive picker, not scriptable.");
                eprintln!("hint: kill the pane and respawn with the model:");
                eprintln!("      codeterm agent spawn codex --model {}", name);
                std::process::exit(1);
            }
            // raw: bypass bracket paste so the TUI parses /model as a slash command, not chat input.
            // Trailing \r submits it — raw mode is byte-for-byte, so we include it.
            let cmd = format!("/model {}\r", name);
            match http
                .post(format!("{}/api/panes/{}/send", base, pane_id))
                .json(&serde_json::json!({"text": cmd, "raw": true}))
                .send()
            {
                Ok(_) => {
                    println!("Sent /model {} to {}", name, pane_id);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn print_agent_mutation_response(resp: reqwest::blocking::Response) {
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    match parse_agent_mutation_response(status, text) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(message) => {
            eprintln!("Error: {message}");
            std::process::exit(1);
        }
    }
}

fn parse_agent_mutation_response(
    status: reqwest::StatusCode,
    text: String,
) -> Result<serde_json::Value, String> {
    let value = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| {
        let message = if text.is_empty() {
            status.to_string()
        } else {
            text
        };
        if status.is_success() {
            serde_json::json!({ "ok": true, "message": message })
        } else {
            serde_json::json!({ "error": message })
        }
    });
    if !status.is_success() {
        let message = value
            .get("error")
            .and_then(|error| error.as_str())
            .unwrap_or_else(|| value.as_str().unwrap_or("agent transition failed"))
            .to_string();
        return Err(message);
    }
    Ok(value)
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
