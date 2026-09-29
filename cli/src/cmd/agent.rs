use crate::args::agent::ReportArgs;
use crate::client::SocketClient;
use crate::cmd::plan::caller_context::CallerContext;
use crate::cmd::plan_errors::render_plan_call_error;
use crate::cmd::print::handle_result;
use crate::{build_local_api, read_api_config, resolve_tab, AgentAction};
use std::io::Read;

#[path = "agent_model_switch.rs"]
mod agent_model_switch;
#[path = "agent_models.rs"]
mod agent_models;
use agent_models::{format_agent_models, format_agent_models_json, provider_availability_marker};
#[cfg(test)]
use agent_models::{AVAILABILITY_UNAVAILABLE, AVAILABILITY_UNKNOWN};

fn absolute_spawn_base(
    spawn_cwd: Option<&str>,
    caller_cwd: &std::path::Path,
) -> std::path::PathBuf {
    match spawn_cwd.map(std::path::Path::new) {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => caller_cwd.join(path),
        None => caller_cwd.to_path_buf(),
    }
}

/// A local spawn's `--cwd` as the daemon must receive it: absolute, resolved
/// against the caller's own directory, which the daemon cannot know.
pub(crate) fn caller_resolved_spawn_cwd(cwd: &str, caller_cwd: Option<&std::path::Path>) -> String {
    match caller_cwd {
        Some(caller_cwd) => absolute_spawn_base(Some(cwd), caller_cwd)
            .to_string_lossy()
            .into_owned(),
        None => cwd.to_string(),
    }
}

/// Resolve the spawn task body from `--task` / `--task-file`. `--task-file`
/// reads the file verbatim (or stdin when the value is `-`), so the body never
/// passes through the caller's shell — backticks and $(...) survive byte-exact.
pub fn resolve_spawn_task<R: Read>(
    task: Option<String>,
    task_file: Option<String>,
    spawn_cwd: Option<&str>,
    stdin: &mut R,
) -> Result<Option<String>, String> {
    let caller_cwd = std::env::current_dir()
        .map_err(|error| format!("failed to resolve caller cwd for --task-file: {error}"))?;
    resolve_spawn_task_from(task, task_file, spawn_cwd, &caller_cwd, stdin)
}

pub(crate) fn resolve_mesh_spawn_task<R: Read>(
    task: Option<String>,
    task_file: Option<String>,
    stdin: &mut R,
) -> Result<Option<String>, String> {
    resolve_spawn_task(task, task_file, None, stdin)
}

fn resolve_spawn_task_from<R: Read>(
    task: Option<String>,
    task_file: Option<String>,
    spawn_cwd: Option<&str>,
    caller_cwd: &std::path::Path,
    stdin: &mut R,
) -> Result<Option<String>, String> {
    match (task, task_file) {
        (Some(_), Some(_)) => Err("pass either --task or --task-file, not both".into()),
        (Some(t), None) => {
            if let Some(refusal) = crate::cmd::quoted_handoff::inline_refusal(
                crate::cmd::quoted_handoff::INLINE_SOURCE,
                &t,
                crate::cmd::quoted_handoff::SPAWN_REMEDY,
            ) {
                return Err(refusal);
            }
            Ok(Some(t))
        }
        (None, Some(path)) => {
            if path == "-" {
                let mut buf = String::new();
                stdin
                    .read_to_string(&mut buf)
                    .map_err(|e| format!("failed to read --task-file from stdin: {}", e))?;
                Ok(Some(buf))
            } else {
                let requested = std::path::Path::new(&path);
                let base = absolute_spawn_base(spawn_cwd, caller_cwd);
                let resolved = if requested.is_absolute() {
                    requested.to_path_buf()
                } else {
                    base.join(requested)
                };
                std::fs::read_to_string(&resolved).map(Some).map_err(|e| {
                    format!(
                        "failed to read --task-file {path} (resolved from {} as {}): {e}",
                        base.display(),
                        resolved.display()
                    )
                })
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

pub(crate) const MAX_TAB_DESCRIPTION_CHARS: usize = 200;

pub(crate) fn join_cli_words(value: Option<Vec<String>>) -> Option<String> {
    value
        .map(|words| words.join(" "))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

mod report_refusal;
use report_refusal::handle_report_refusal;

pub(crate) fn normalize_tab_description(value: &str) -> Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.chars().count() > MAX_TAB_DESCRIPTION_CHARS {
        return Err(format!(
            "description must be {} characters or fewer",
            MAX_TAB_DESCRIPTION_CHARS
        ));
    }
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn env_default(current: Option<String>, var: &str) -> Option<String> {
    current
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var(var)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

fn missing_report_binding_incident(tab_id: &str) -> String {
    format!(
        "ORCHESTRATION AUTHORITY REFUSED: tab '{tab_id}' has no explicit plan/task assignment.\n\
         Impact: this mutation cannot be attached to an authoritative task safely.\n\
         Required: assign the tab or pass explicit --plan and --task; no plan or task was guessed."
    )
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
    if let Some(diagnostics) = val.get("diagnostics") {
        let errors = diagnostics
            .get("error_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let warnings = diagnostics
            .get("warning_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        eprintln!("diagnostics: {errors} errors, {warnings} warnings");
        if let Some(findings) = diagnostics
            .get("findings")
            .and_then(|value| value.as_array())
        {
            for finding in findings.iter().take(3) {
                let file = finding
                    .get("file")
                    .and_then(|value| value.as_str())
                    .unwrap_or("<repo>");
                let line = finding
                    .get("line")
                    .and_then(|value| value.as_u64())
                    .map(|value| format!(":{value}"))
                    .unwrap_or_default();
                let message = finding
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                eprintln!("diagnostic: {file}{line}: {message}");
            }
        }
    }
}

pub fn handle_agent(client: &mut SocketClient, json: bool, action: AgentAction) {
    let (base, _) = read_api_config();
    let local_http = match build_local_api() {
        Ok(client) => client,
        Err(error) => {
            crate::cmd::print::fail(1, format!("failed to connect to local API: {error}"));
        }
    };
    match action {
        AgentAction::Providers => match local_http
            .get(format!("{}/api/agents/providers", base))
            .send()
        {
            Ok(resp) => {
                let status = resp.status();
                let val: serde_json::Value = resp.json().unwrap_or_default();
                if !status.is_success() {
                    let body = if val.is_null() {
                        String::new()
                    } else {
                        val.to_string()
                    };
                    crate::cmd::print::fail_http(1, status.as_u16(), &body);
                }
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    return;
                }
                let Some(arr) = val.as_array() else {
                    crate::cmd::print::fail(
                        1,
                        format!("Unexpected response shape (HTTP {})", status),
                    );
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
                        "{:<12} {:<6} {:<20} default={}  models={}{}",
                        id,
                        kind,
                        name,
                        default_model,
                        models_count,
                        provider_availability_marker(p)
                    );
                }
            }
            Err(e) => {
                crate::cmd::print::fail(1, e.to_string());
            }
        },
        AgentAction::Models { provider } => {
            match local_http
                .get(format!("{}/api/agents/providers", base))
                .send()
            {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    let Some(arr) = val.as_array() else {
                        crate::cmd::print::fail(1, "Unexpected response shape");
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
                        println!("{}", format_agent_models_json(target));
                        return;
                    }
                    print!("{}", format_agent_models(target, &provider));
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::SetOrchestrator { tab } => {
            let tab_id = resolve_tab(client, tab);
            match local_http
                .post(format!("{}/api/tabs/{}/role", base, tab_id))
                .json(&serde_json::json!({"role": "Orchestrator"}))
                .send()
            {
                Ok(resp) => print_local_agent_mutation_response(resp),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::Link { parent, tab } => {
            let tab_id = resolve_tab(client, tab);
            let parent_resp = local_http
                .get(format!("{}/api/tabs/{}/status", base, parent))
                .send();
            let role = match parent_resp {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    let anchors = val
                        .get("role_profile")
                        .and_then(|v| v.as_str())
                        .is_some_and(|kind| kind == "orchestrator" || kind == "general_agent");
                    if anchors {
                        "Worker"
                    } else {
                        crate::cmd::print::fail(1, "Parent must be an Orchestrator");
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            };
            match local_http
                .post(format!("{}/api/tabs/{}/role", base, tab_id))
                .json(&serde_json::json!({"role": role, "parent_tab_id": parent}))
                .send()
            {
                Ok(resp) => print_local_agent_mutation_response(resp),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::Unlink { tab } => {
            let tab_id = resolve_tab(client, tab);
            match local_http
                .delete(format!("{}/api/tabs/{}/role", base, tab_id))
                .send()
            {
                Ok(resp) => print_local_agent_mutation_response(resp),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::AllowSpawn { tab, enable } => {
            let tab_id = resolve_tab(client, tab);
            match local_http
                .post(format!("{}/api/tabs/{}/can-spawn", base, tab_id))
                .json(&serde_json::json!({"can_spawn": enable}))
                .send()
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp.json().unwrap_or_default();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::Report(ReportArgs {
            message,
            text,
            kind,
            doc,
            from_pad,
            tab,
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
            if let Some(ref explicit) = tab {
                if let Ok(own) = std::env::var("CODETERM_TAB_ID") {
                    let differs = !own.is_empty()
                        && own != *explicit
                        && !own.starts_with(explicit.as_str())
                        && !explicit.starts_with(own.as_str());
                    if differs {
                        crate::cmd::print::fail(
                            1,
                            format!(
                                "`agent report --tab {explicit}` reports AS that tab, not TO it.\n\
                                 Reports route to your parent automatically — drop --tab to report up the hierarchy.\n\
                                 To send a message to a specific tab, use `codeterm send --tab {explicit}`."
                            ),
                        );
                    }
                }
            }
            crate::cmd::payload_source::refuse_inline_report_body(
                text.as_deref().or(message.as_deref()),
            );
            let tab_id = resolve_tab(client, tab);
            let message = crate::cmd::report_body::read_report_body(message, text, from_pad, doc);
            if message.is_empty() {
                crate::cmd::print::fail(1, "report message, --text, or --doc is required");
            }
            let resolved_echo =
                (!json).then(|| crate::cmd::resolved_text::resolved_text_echo(&message));
            let plan = env_default(plan, "CODETERM_PLAN_ID");
            let task = env_default(task, "CODETERM_TASK_ID");
            let task_bound = status.is_some()
                && crate::cmd::report_guard::report_task_binding_resolved(
                    client,
                    plan.as_deref(),
                    task.as_deref(),
                    &tab_id,
                );
            if status.is_some()
                && !crate::cmd::report_guard::worker_binding_resolved(
                    false,
                    plan.is_some(),
                    task_bound,
                )
            {
                let tab = std::env::var("CODETERM_TAB_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| tab_id.clone());
                crate::cmd::print::fail(1, missing_report_binding_incident(&tab));
            }
            let mut body = serde_json::json!({
                "tab_id": tab_id,
                "caller_tab_id": tab_id,
                "message": message,
            });
            if let Ok(session_id) = std::env::var("CODETERM_SESSION_ID") {
                body["caller_session_id"] = serde_json::json!(session_id);
            }
            if let Ok(nonce) = std::env::var("CODETERM_SESSION_BINDING_NONCE") {
                body["session_binding_nonce"] = serde_json::json!(nonce);
            }
            if let Ok(attempt_id) = std::env::var("CODETERM_ATTEMPT_ID") {
                body["attempt_id"] = serde_json::json!(attempt_id);
            }
            if let Some(k) = kind {
                body["kind"] = serde_json::json!(k);
            }
            if let Some(ref p) = plan {
                body["plan_id"] = serde_json::json!(p);
            }
            if let Some(ref t) = task {
                body["task_id"] = serde_json::json!(t);
            }
            if let (Some(ref p), Some(ref t)) = (&plan, &task) {
                let shown = client.call_detailed(
                    "task.show",
                    Some(serde_json::json!({ "plan_id": p, "task_id": t })),
                );
                if let Ok(shown) = shown {
                    if let Some(epoch) = shown
                        .get("task")
                        .unwrap_or(&shown)
                        .get("interruption")
                        .and_then(|value| value.get("owner_epoch"))
                        .and_then(|value| value.as_u64())
                    {
                        body["owner_epoch"] = serde_json::json!(epoch);
                    }
                }
            }
            if let Some(s) = status {
                body["status"] = serde_json::json!(s.as_str());
            }
            let task_scope = if crate::cmd::report_guard::is_readiness_status(status) {
                crate::cmd::report_lane::task_readiness_scope(
                    client,
                    plan.as_deref(),
                    task.as_deref(),
                    &tab_id,
                )
            } else {
                None
            };
            let canonical_writer = task_scope
                .as_ref()
                .is_some_and(|scope| scope.canonical_writer);
            let base_for_report = if canonical_writer {
                None
            } else {
                base.clone()
                    .or_else(|| std::env::var("CODETERM_LANE_BASE").ok())
            };
            let report_base = if canonical_writer {
                crate::cmd::report_attribution::ReportBase::default()
            } else {
                crate::cmd::report_attribution::ReportBase::classify(
                    base,
                    std::env::var("CODETERM_LANE_BASE").ok(),
                )
            };
            let (branch, sha, changed_files) = match crate::cmd::report_guard::enrich_ready_report(
                task_scope.as_ref(),
                status,
                branch,
                sha,
                report_base,
                changed_files,
                acknowledge_dirty,
            ) {
                Ok(fields) => fields,
                Err(refusal) => handle_report_refusal(client, json, body, refusal),
            };
            if acknowledge_dirty && !canonical_writer {
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
                                "from": CallerContext::from_env().owner(),
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
            if let Some(b) = base_for_report {
                body["base"] = serde_json::json!(b);
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
            let delivered = result.is_ok();
            handle_result(json, result);
            if delivered {
                if let Some(line) = resolved_echo {
                    println!("{line}");
                }
            }
        }
        AgentAction::Ack { tab, plan, task } => {
            let tab_id = super::local_io::resolve_caller_tab(tab, "agent ack");
            let plan_id = env_default(plan, "CODETERM_PLAN_ID");
            let task_id = env_default(task, "CODETERM_TASK_ID");
            let mut body = serde_json::json!({ "tab_id": tab_id });
            if let Some(plan_id) = plan_id {
                body["plan_id"] = serde_json::json!(plan_id);
            }
            if let Some(task_id) = task_id {
                body["task_id"] = serde_json::json!(task_id);
            }
            let result = client.call("agent.ack", Some(body));
            match result {
                Ok(value) if json => {
                    println!("{}", serde_json::to_string_pretty(&value).unwrap());
                }
                Ok(value) => match render_agent_ack(&value) {
                    Ok(message) => println!("{message}"),
                    Err(error) => {
                        crate::cmd::print::fail(1, format!("ACK NOT ACCEPTED: {error}"));
                    }
                },
                Err(error) => {
                    crate::cmd::print::fail(1, format!("ACK NOT ACCEPTED: {error}"));
                }
            }
        }
        AgentAction::StartupAck => {
            let tab_id = super::local_io::resolve_caller_tab(None, "agent startup-ack");
            let required_env = |name: &str| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| {
                        crate::cmd::print::fail(
                            1,
                            format!("STARTUP ACK NOT ACCEPTED: {name} is not set"),
                        )
                    })
            };
            let attempt_id = required_env("CODETERM_STARTUP_ATTEMPT_ID");
            let binding_nonce = required_env("CODETERM_SESSION_BINDING_NONCE");
            handle_result(
                json,
                client.call(
                    "agent.startup_ack",
                    Some(serde_json::json!({
                        "caller_tab_id": tab_id,
                        "attempt_id": attempt_id,
                        "binding_nonce": binding_nonce,
                    })),
                ),
            );
        }
        AgentAction::StartupHold { tab, attempt } => {
            let caller_tab_id = super::local_io::resolve_caller_tab(None, "agent startup-hold");
            let result = client.call(
                "agent.startup_hold",
                Some(serde_json::json!({
                    "caller_tab_id": caller_tab_id,
                    "tab_id": tab,
                    "attempt_id": attempt,
                })),
            );
            if json {
                handle_result(json, result);
                return;
            }
            match result {
                Ok(value) => println!("{}", render_startup_hold(&value)),
                Err(error) => {
                    crate::cmd::print::fail(1, format!("STARTUP HOLD REFUSED: {error}"));
                }
            }
        }
        action @ AgentAction::Spawn { .. } => super::agent_spawn::handle(client, json, action),
        AgentAction::Ledger { tab } => {
            let tab_id = resolve_tab(client, tab);
            match client.call(
                "agent.ledger",
                Some(serde_json::json!({ "tab_id": tab_id })),
            ) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No workers in ledger for {}.", tab_id);
                        }
                        for r in arr {
                            let worker = r
                                .get("worker_tab_id")
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
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::Compact { tab } => {
            let tab_id = resolve_tab(client, tab);
            let cmd = "/compact\n";
            match local_http
                .post(format!("{}/api/tabs/{}/send", base, tab_id))
                .json(&serde_json::json!({"text": cmd}))
                .send()
            {
                Ok(_) => {
                    println!("Sent /compact to {}", tab_id);
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::Restart {
            tab,
            force,
            grace_secs,
        } => {
            let tab_id = resolve_tab(client, tab);
            let body = serde_json::json!({"force": force, "grace_secs": grace_secs});
            match local_http
                .post(format!("{}/api/tabs/{}/agent/restart", base, tab_id))
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
                        crate::cmd::print::fail(
                            1,
                            format!("rate limited (retry after {}s)", retry),
                        );
                    }
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    match restart_response_outcome(status, &val) {
                        Ok(()) => {
                            if json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("{}", render_restart_success(&val, &tab_id));
                            }
                        }
                        Err(msg) => {
                            crate::cmd::print::fail(1, msg.to_string());
                        }
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        AgentAction::Model { name, tab } => {
            let tab_id = resolve_tab(client, tab);
            let result = local_http
                .post(format!("{}/api/tabs/{}/agent/model", base, tab_id))
                .json(&serde_json::json!({ "model": name }))
                .send()
                .map_err(|error| crate::cmd::print::CliError::new(error.to_string()))
                .and_then(|resp| {
                    let status = resp.status();
                    let text = resp.text().unwrap_or_default();
                    agent_model_switch::model_switch_result(status, &text)
                });
            match result {
                Ok(value) if json => crate::cmd::print::print_json(&value),
                Ok(value) => println!("{}", agent_model_switch::render_model_switch(&value)),
                Err(error) => crate::cmd::print::exit_with_cli_error(json, &error),
            }
        }
    }
}

fn render_agent_ack(value: &serde_json::Value) -> Result<String, String> {
    let task_id = value
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "daemon response omitted task identity".to_string())?;
    let attempt_id = value
        .get("attempt_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "daemon response omitted attempt identity".to_string())?;
    if value
        .get("already_acknowledged")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Ok(format!(
            "ALREADY ACKNOWLEDGED — task {task_id}, attempt {attempt_id}. Continue work."
        ));
    }
    Ok(format!(
        "ACKNOWLEDGED — task {task_id}, attempt {attempt_id}. Continue work."
    ))
}

fn render_startup_hold(value: &serde_json::Value) -> String {
    let field = |key: &str| {
        value
            .pointer(&format!("/authority/{key}"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("-")
            .to_string()
    };
    format!(
        "STARTUP HOLD ACCEPTED — tab {}, attempt {}, creator {}, team {}.",
        field("child_tab_id"),
        field("attempt_id"),
        field("creator_tab_id"),
        field("team_id"),
    )
}

/// Success (exit 0) vs. failure (exit 1); the unreported-vs-foreign-session
/// distinction is already resolved server-side by the time `status` arrives.
fn restart_response_outcome(
    status: reqwest::StatusCode,
    val: &serde_json::Value,
) -> Result<(), String> {
    if status.is_success() {
        return Ok(());
    }
    let msg = val
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| val.as_str().unwrap_or("restart failed"));
    Err(msg.to_string())
}

fn render_restart_success(val: &serde_json::Value, tab_id: &str) -> String {
    let action = val
        .get("session_action")
        .and_then(|v| v.as_str())
        .unwrap_or("fresh");
    let full_access = val
        .get("full_access")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sid = val.get("session_id").and_then(|v| v.as_str());
    let session = match (action, sid) {
        ("resume", Some(s)) => format!("resume {}", s),
        _ => "fresh session".to_string(),
    };
    format!(
        "Restarting Tab {}: {}, full access={} (same Tab id, same session unless stale)",
        tab_id,
        session,
        if full_access { "on" } else { "off" }
    )
}

fn print_local_agent_mutation_response(resp: crate::local_api::LocalApiResponse) {
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    match parse_agent_mutation_response(status, text) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(message) => {
            crate::cmd::print::fail(1, message.to_string());
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
