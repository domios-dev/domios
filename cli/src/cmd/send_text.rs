use crate::client::SocketClient;
use crate::cmd::print::print_human;
use crate::cmd::tab::remote_target::parse_remote_tab_id;
use crate::resolve_tab;
use regex::Regex;

const RESOLVED_TEXT_PREFIX: &str = "CODETERM_RESOLVED_TEXT=";

/// Detect content that's almost certainly the result of bash command-substitution
/// eating the agent's intended text — e.g. unquoted `mesh peers` inside a
/// double-quoted argv string causes bash to execute `mesh peers` and splice the
/// resulting "command not found" stderr into the payload before send sees
/// any of it.
pub fn guard_against_shell_mangle(text: &str) -> Result<(), String> {
    const SUSPICIOUS: &[&str] = &[
        ": command not found",
        "/usr/bin/bash: line ",
        "bash: line ",
        "syntax error near unexpected token",
    ];
    let scrubbed = strip_fenced_code_blocks(text);
    for needle in SUSPICIOUS {
        if scrubbed.contains(needle) {
            return Err(format!(
                "send payload looks shell-mangled (matched marker: {:?})",
                needle
            ));
        }
    }
    Ok(())
}

fn strip_fenced_code_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_block = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if !in_block {
            out.push_str(line);
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendTextOutcome {
    Success(String),
    Error { msg: String, exit_code: i32 },
    Other,
}

pub fn analyze_send_text_response(value: &serde_json::Value) -> SendTextOutcome {
    if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
        return SendTextOutcome::Error {
            msg: err.to_string(),
            exit_code: 2,
        };
    }
    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return SendTextOutcome::Success("OK (sent)".to_string());
    }
    SendTextOutcome::Other
}

pub fn redact_resolved_send_text(text: &str) -> String {
    let bearer = Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}").unwrap();
    let assignment = Regex::new(r"(?i)\b([a-z][a-z0-9_]*(?:token|secret|password|api[_-]?key|credential)[a-z0-9_]*)=([^\s]+)").unwrap();
    let openai_key = Regex::new(r"\bsk-[A-Za-z0-9_-]{8,}\b").unwrap();
    let redacted = bearer.replace_all(text, "$1***REDACTED***");
    let redacted = assignment.replace_all(&redacted, "$1=***REDACTED***");
    openai_key
        .replace_all(&redacted, "***REDACTED***")
        .into_owned()
}

pub fn resolved_text_echo(text: &str) -> String {
    format!(
        "{}{}",
        RESOLVED_TEXT_PREFIX,
        serde_json::to_string(&redact_resolved_send_text(text)).unwrap()
    )
}

pub fn handle_send_text_result(
    json: bool,
    result: Result<serde_json::Value, String>,
    resolved_text: Option<&str>,
) {
    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                return;
            }
            match analyze_send_text_response(&value) {
                SendTextOutcome::Success(msg) => {
                    println!("{}", msg);
                    if let Some(text) = resolved_text {
                        println!("{}", resolved_text_echo(text));
                    }
                }
                SendTextOutcome::Error { msg, exit_code } => {
                    eprintln!("Error: {}", msg);
                    std::process::exit(exit_code);
                }
                SendTextOutcome::Other => print_human(&value),
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Agent,
    RawShell,
    ChatBackend,
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendEligibility {
    Allow,
    Refuse(String),
}

/// Prose/task-charter shape: multiline or beyond the inline budget. A short
/// single-line command (e.g. `git status`) stays eligible for a raw shell.
pub fn is_task_shaped_payload(text: &str) -> bool {
    const INLINE_BUDGET: usize = 240;
    text.contains('\n') || text.chars().count() > INLINE_BUDGET
}

pub fn classify_tab_kind(record: &serde_json::Value) -> TabKind {
    let has_agent = record
        .get("agent_type")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    if has_agent {
        return TabKind::Agent;
    }
    match record.get("backing_kind").and_then(|v| v.as_str()) {
        Some("chat_backend") => TabKind::ChatBackend,
        Some("pty") => TabKind::RawShell,
        _ => TabKind::Unknown,
    }
}

/// Caller-side eligibility decision — reads existing tab-record state, never
/// touches byte delivery. Refuses task-shaped prose bound for a raw shell so a
/// non-agent tab no longer silently executes a charter line by line (issue #8).
pub fn decide_send_eligibility(
    kind: TabKind,
    text: &str,
    require_agent: bool,
    to_shell: bool,
) -> SendEligibility {
    if to_shell {
        return SendEligibility::Allow;
    }
    match kind {
        TabKind::Agent => SendEligibility::Allow,
        TabKind::ChatBackend => {
            if require_agent {
                SendEligibility::Refuse(
                    "--require-agent: target is a chat-backend tab, not a PTY agent".into(),
                )
            } else {
                SendEligibility::Allow
            }
        }
        TabKind::RawShell => {
            if require_agent {
                SendEligibility::Refuse(refuse_raw_shell_message(true))
            } else if is_task_shaped_payload(text) {
                SendEligibility::Refuse(refuse_raw_shell_message(false))
            } else {
                SendEligibility::Allow
            }
        }
        TabKind::Unknown => {
            if require_agent {
                SendEligibility::Refuse(
                    "--require-agent: could not confirm an agent is attached to the target tab"
                        .into(),
                )
            } else {
                SendEligibility::Allow
            }
        }
    }
}

fn refuse_raw_shell_message(require_agent: bool) -> String {
    if require_agent {
        "target tab is a raw shell (no agent attached); --require-agent refuses to type into a shell"
            .into()
    } else {
        "refusing to send prose to a raw shell tab: the shell would execute it line by line (issue #8). \
         Pass --to-shell (alias --force) to deliver raw text to the shell, or target an agent tab."
            .into()
    }
}

fn probe_tab_kind(client: &mut SocketClient, tab_id: &str) -> TabKind {
    let params = serde_json::json!({ "focused_workspace": false });
    match client.call("tab.list", Some(params)) {
        Ok(value) => value
            .as_array()
            .and_then(|tabs| {
                tabs.iter().find(|tab| {
                    tab.get("id").and_then(|v| v.as_str()) == Some(tab_id)
                        || tab.get("tab_id").and_then(|v| v.as_str()) == Some(tab_id)
                })
            })
            .map(classify_tab_kind)
            .unwrap_or(TabKind::Unknown),
        Err(_) => TabKind::Unknown,
    }
}

fn emit_send_refusal(json: bool, msg: &str) -> ! {
    if json {
        let err = serde_json::json!({ "ok": false, "refused": true, "error": msg });
        println!("{}", serde_json::to_string_pretty(&err).unwrap());
    } else {
        eprintln!("Error: {}", msg);
    }
    std::process::exit(2);
}

#[allow(
    clippy::too_many_arguments,
    reason = "CLI command preserves send text option surface."
)]
pub fn handle_send_text(
    client: &mut SocketClient,
    json: bool,
    tab: Option<String>,
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    from_stdin: bool,
    actions: Option<String>,
    delay: Option<String>,
    at: Option<String>,
    require_agent: bool,
    to_shell: bool,
) {
    if let Some((peer, remote_tab)) = tab.as_deref().and_then(parse_remote_tab_id) {
        if require_agent {
            emit_send_refusal(
                json,
                "--require-agent cannot verify a remote tab's agent over mesh; run send on the owning peer",
            );
        }
        send_composite_over_mesh(
            json,
            &peer,
            &remote_tab,
            text,
            from_file,
            from_pad,
            from_stdin,
            actions,
            delay,
            at,
        );
        return;
    }
    if delay.is_some() || at.is_some() {
        if actions.is_some() {
            eprintln!("Error: --actions cannot be queued with --delay/--at");
            std::process::exit(2);
        }
        crate::cmd::delayed::queue_from_send(
            client, json, tab, text, from_file, from_pad, from_stdin, delay, at,
        );
        return;
    }
    let mut stdin = std::io::stdin();
    let (resolved_text, parsed_actions) =
        resolve_and_validate_send(text, from_file, from_pad, from_stdin, actions, &mut stdin);
    let tab_id = resolve_tab(client, tab);
    if !resolved_text.trim().is_empty() {
        let kind = probe_tab_kind(client, &tab_id);
        if let SendEligibility::Refuse(msg) =
            decide_send_eligibility(kind, &resolved_text, require_agent, to_shell)
        {
            emit_send_refusal(json, &msg);
        }
    }
    if let Some(actions) = parsed_actions {
        let result = send_actions_via_socket(json, &tab_id, &resolved_text, actions);
        handle_send_text_result(json, result, Some(&resolved_text));
        return;
    }
    let mut params = serde_json::json!({
        "tab_id": tab_id,
        "text": resolved_text,
    });
    // recipient side. Skip when target is self (chatting with own tab — no point self-tagging).
    if let Ok(my_tab) = std::env::var("CODETERM_TAB_ID") {
        if !my_tab.is_empty() && my_tab != tab_id {
            params["source_tab"] = serde_json::json!(my_tab);
        }
    }
    let result = client.call("tab.send_text", Some(params));
    handle_send_text_result(json, result, Some(&resolved_text));
}

fn resolve_and_validate_send<R: std::io::Read>(
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    from_stdin: bool,
    actions: Option<String>,
    stdin: &mut R,
) -> (String, Option<Vec<SendActionSpec>>) {
    let (resolved_text, source) =
        match resolve_send_text_input(text, from_file, from_pad, from_stdin, stdin) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        };
    let is_empty = resolved_text.trim().is_empty();
    let parsed_actions = actions
        .as_deref()
        .map(parse_actions_spec)
        .transpose()
        .unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(2);
        });
    if is_empty && parsed_actions.is_none() {
        eprintln!("Error: send payload is empty (source: {})", source);
        eprintln!("hint: if using --stdin, ensure the heredoc actually delivered content;");
        eprintln!("      if --from-file, verify the path exists and isn't 0 bytes;");
        eprintln!("      if positional, your shell may have eaten the content.");
        eprintln!("      to send a bare Enter, pass --actions enter.");
        std::process::exit(2);
    }
    if !is_empty {
        if let Err(msg) = guard_against_shell_mangle(&resolved_text) {
            eprintln!("Error: {}", msg);
            eprintln!(
                "hint: your shell expanded a command-substitution before codeterm-CLI received it."
            );
            eprintln!("      Use --from-file <path> or --from-stdin <<'EOF' ... EOF for content with backticks/$/code.");
            std::process::exit(2);
        }
    }
    (resolved_text, parsed_actions)
}

pub fn remote_send_body(
    text: &str,
    actions: Option<&[SendActionSpec]>,
    source_tab: Option<&str>,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    if !text.is_empty() {
        body.insert("text".into(), serde_json::json!(text));
    }
    if let Some(actions) = actions {
        body.insert("actions".into(), actions_to_json(actions));
    }
    if let Some(source) = source_tab.filter(|s| !s.is_empty()) {
        body.insert("source_tab".into(), serde_json::json!(source));
    }
    serde_json::Value::Object(body)
}

#[allow(
    clippy::too_many_arguments,
    reason = "mirrors the send option surface for the mesh relay path."
)]
fn send_composite_over_mesh(
    json: bool,
    peer: &str,
    remote_tab: &str,
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    from_stdin: bool,
    actions: Option<String>,
    delay: Option<String>,
    at: Option<String>,
) {
    if delay.is_some() || at.is_some() {
        eprintln!("Error: delayed send to a remote tab is unsupported; deliver now or run on the owning peer");
        std::process::exit(2);
    }
    let mut stdin = std::io::stdin();
    let (resolved_text, parsed_actions) =
        resolve_and_validate_send(text, from_file, from_pad, from_stdin, actions, &mut stdin);
    let source_tab = std::env::var("CODETERM_TAB_ID").ok();
    let body = remote_send_body(
        &resolved_text,
        parsed_actions.as_deref(),
        source_tab.as_deref(),
    );
    let path = format!("/api/tabs/{}/send", urlencoding::encode(remote_tab));
    let data = crate::mesh_cli::mesh_relay_fetch_json(peer, "POST", &path, Some(&body.to_string()));
    handle_send_text_result(json, Ok(data), Some(&resolved_text));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendActionSpec {
    Key(String),
    Delay(u64),
}

pub fn parse_actions_spec(spec: &str) -> Result<Vec<SendActionSpec>, String> {
    let tokens = split_action_tokens(spec);
    if tokens.is_empty() {
        return Err("--actions is empty".into());
    }
    let mut out = Vec::new();
    for token in tokens {
        let (base, repeat) = parse_repeat(&token)?;
        let action = parse_action_token(base)?;
        match action {
            SendActionSpec::Delay(_) if repeat != 1 => {
                return Err("delay actions cannot be repeated with *N".into());
            }
            SendActionSpec::Delay(ms) => out.push(SendActionSpec::Delay(ms)),
            SendActionSpec::Key(key) => {
                for _ in 0..repeat {
                    out.push(SendActionSpec::Key(key.clone()));
                }
            }
        }
    }
    Ok(out)
}

pub fn actions_to_json(actions: &[SendActionSpec]) -> serde_json::Value {
    serde_json::Value::Array(
        actions
            .iter()
            .map(|action| match action {
                SendActionSpec::Key(key) => serde_json::json!({"type": "key", "key": key}),
                SendActionSpec::Delay(ms) => serde_json::json!({"type": "delay", "ms": ms}),
            })
            .collect(),
    )
}

fn split_action_tokens(spec: &str) -> Vec<String> {
    let raw: Vec<&str> = if spec.contains(',') {
        spec.split(',').collect()
    } else {
        spec.split_whitespace().collect()
    };
    raw.into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_repeat(token: &str) -> Result<(&str, u32), String> {
    let Some((base, repeat)) = token.rsplit_once('*') else {
        return Ok((token, 1));
    };
    let repeat = repeat
        .parse::<u32>()
        .map_err(|_| format!("invalid action repeat: {}", token))?;
    if repeat == 0 || repeat > 100 {
        return Err("action repeat must be between 1 and 100".into());
    }
    let base = base.trim();
    if base.is_empty() {
        return Err(format!("invalid action repeat: {}", token));
    }
    Ok((base, repeat))
}

fn parse_action_token(token: &str) -> Result<SendActionSpec, String> {
    let token = token.trim();
    let lower = token.to_ascii_lowercase();
    if let Some(value) = lower
        .strip_prefix("delay:")
        .or_else(|| lower.strip_prefix("delay="))
        .or_else(|| lower.strip_prefix("wait:"))
        .or_else(|| lower.strip_prefix("wait="))
        .or_else(|| lower.strip_prefix("sleep:"))
        .or_else(|| lower.strip_prefix("sleep="))
    {
        return parse_action_delay(value).map(SendActionSpec::Delay);
    }
    if lower.as_bytes().first().is_some_and(|b| b.is_ascii_digit())
        && (lower.ends_with("ms") || lower.ends_with('s'))
    {
        return parse_action_delay(&lower).map(SendActionSpec::Delay);
    }
    let key = canonical_key_name(token)?;
    Ok(SendActionSpec::Key(key))
}

fn parse_action_delay(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("delay action is empty".into());
    }
    if let Some(ms) = trimmed.strip_suffix("ms") {
        return ms
            .parse::<u64>()
            .map_err(|_| format!("invalid delay action: {}", value));
    }
    if let Some(sec) = trimmed.strip_suffix('s') {
        let seconds = sec
            .parse::<u64>()
            .map_err(|_| format!("invalid delay action: {}", value))?;
        return Ok(seconds.saturating_mul(1000));
    }
    trimmed
        .parse::<u64>()
        .map_err(|_| format!("invalid delay action: {}", value))
}

fn canonical_key_name(key: &str) -> Result<String, String> {
    let normalized = key.replace('_', "-");
    let lower = normalized.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "enter" | "return" => "enter",
        "esc" | "escape" => "esc",
        "tab" => "tab",
        "backspace" | "bs" => "backspace",
        "delete" | "del" => "delete",
        "up" | "arrow-up" => "up",
        "down" | "arrow-down" => "down",
        "left" | "arrow-left" => "left",
        "right" | "arrow-right" => "right",
        "home" => "home",
        "end" => "end",
        "pageup" | "page-up" | "pgup" => "pageup",
        "pagedown" | "page-down" | "pgdn" => "pagedown",
        "space" => "space",
        other if parse_ctrl_key(other).is_some() => other,
        _ if key.chars().count() == 1 => return Ok(key.to_string()),
        _ => {
            return Err(format!(
                "unknown action key '{}'; use a literal character, enter, esc, tab, backspace, delete, arrows, home/end, pageup/pagedown, space, ctrl-c",
                key
            ))
        }
    };
    Ok(mapped.to_string())
}

#[cfg(test)]
fn key_action_bytes(key: &str) -> Option<Vec<u8>> {
    if key.chars().count() == 1 {
        return Some(key.as_bytes().to_vec());
    }
    let normalized = key.replace('_', "-").to_ascii_lowercase();
    match normalized.as_str() {
        "enter" | "return" => Some(b"\r".to_vec()),
        "esc" | "escape" => Some(b"\x1b".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "backspace" | "bs" => Some(vec![0x7f]),
        "delete" | "del" => Some(b"\x1b[3~".to_vec()),
        "up" | "arrow-up" => Some(b"\x1b[A".to_vec()),
        "down" | "arrow-down" => Some(b"\x1b[B".to_vec()),
        "right" | "arrow-right" => Some(b"\x1b[C".to_vec()),
        "left" | "arrow-left" => Some(b"\x1b[D".to_vec()),
        "home" => Some(b"\x1b[H".to_vec()),
        "end" => Some(b"\x1b[F".to_vec()),
        "pageup" | "page-up" | "pgup" => Some(b"\x1b[5~".to_vec()),
        "pagedown" | "page-down" | "pgdn" => Some(b"\x1b[6~".to_vec()),
        "space" => Some(b" ".to_vec()),
        other => parse_ctrl_key(other).map(|b| vec![b]),
    }
}

fn parse_ctrl_key(value: &str) -> Option<u8> {
    let key = value
        .strip_prefix("ctrl-")
        .or_else(|| value.strip_prefix("control-"))
        .or_else(|| value.strip_prefix("c-"))?;
    if key.len() != 1 {
        return None;
    }
    let b = key.as_bytes()[0].to_ascii_lowercase();
    if b.is_ascii_lowercase() {
        Some(b - b'a' + 1)
    } else if b == b'[' {
        Some(0x1b)
    } else {
        None
    }
}

fn send_actions_via_socket(
    _json: bool,
    tab_id: &str,
    text: &str,
    actions: Vec<SendActionSpec>,
) -> Result<serde_json::Value, String> {
    let (base, _) = crate::read_api_config();
    let api = crate::build_local_api().map_err(|e| e.to_string())?;
    let mut body = serde_json::json!({
        "actions": actions_to_json(&actions),
    });
    if !text.is_empty() {
        body["text"] = serde_json::json!(text);
    }
    if let Ok(my_tab) = std::env::var("CODETERM_TAB_ID") {
        if !my_tab.is_empty() && my_tab != tab_id {
            body["source_tab"] = serde_json::json!(my_tab);
        }
    }
    let resp = api
        .post(format!("{}/api/tabs/{}/send", base, tab_id))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, text.trim()));
    }
    if text.trim().is_empty() {
        return Ok(serde_json::json!({"ok": true}));
    }
    let value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(value)
}

pub fn resolve_send_text_input<R: std::io::Read>(
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    from_stdin: bool,
    stdin: &mut R,
) -> Result<(String, &'static str), String> {
    if let Some(path) = from_pad {
        return crate::cmd::payload_source::read_from_pad(&path).map(|s| (s, "--from-pad"));
    }
    if from_stdin {
        let mut buf = String::new();
        stdin
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read --stdin: {}", e))?;
        return Ok((buf, "--stdin"));
    }
    if let Some(path) = from_file {
        return std::fs::read_to_string(&path)
            .map(|s| (s, "--from-file"))
            .map_err(|e| format!("failed to read --from-file {}: {}", path, e));
    }
    let text = text.unwrap_or_default();
    if let Some(hint) = pad_nudge("positional", &text) {
        eprintln!("{hint}");
    }
    Ok((text, "positional"))
}

/// Soft nudge toward the pad when an authored payload was inlined. Advisory
/// only — a short scalar argument stays inline, so the shape of the payload
/// decides, not the caller.
pub fn pad_nudge(source: &str, text: &str) -> Option<String> {
    staging_nudge(
        source,
        text,
        "Stage it in the pad (`codeterm pad path`) and pass --from-pad.",
    )
}

/// Same heavy-inline guard as `pad_nudge`; the caller supplies the staging
/// remedy naming the flag valid for its command (spawn: --task-file/--plan-task).
pub fn staging_nudge(source: &str, text: &str, remedy: &str) -> Option<String> {
    const INLINE_BUDGET: usize = 240;
    if source != "positional" {
        return None;
    }
    let multiline = text.contains('\n');
    if !multiline && text.chars().count() <= INLINE_BUDGET {
        return None;
    }
    let shape = if multiline {
        "multiline"
    } else {
        "over 240 characters"
    };
    Some(format!(
        "hint: this payload is {shape} and went inline — the shell mangles backticks, $, quotes, \
         and newlines on the way. {remedy}"
    ))
}

/// Print the report-body staging nudge to stderr (skipped for `--json`).
pub fn nudge_report_body(json: bool, text: &str) {
    emit_staging_nudge(
        json,
        text,
        "Stage it in the pad (`codeterm pad path`) and pass --from-pad.",
    );
}

/// Print the spawn-task staging nudge naming the flags spawn actually has.
pub fn nudge_spawn_task(json: bool, text: &str) {
    emit_staging_nudge(
        json,
        text,
        "Pass it byte-exact with --task-file <path>, or bind a plan task via --plan-task <id> — both bypass the shell.",
    );
}

fn emit_staging_nudge(json: bool, text: &str, remedy: &str) {
    if json {
        return;
    }
    if let Some(hint) = staging_nudge("positional", text, remedy) {
        eprintln!("{hint}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_send_text_response, classify_tab_kind, decide_send_eligibility,
        guard_against_shell_mangle, is_task_shaped_payload, key_action_bytes, parse_actions_spec,
        redact_resolved_send_text, resolve_send_text_input, resolved_text_echo, SendActionSpec,
        SendEligibility, SendTextOutcome, TabKind,
    };

    const CHARTER: &str = "You are Worker Leo.\nExecute the task; report on completion.";

    #[test]
    fn classify_agent_tab_from_agent_type() {
        let record = serde_json::json!({"id": "t1", "agent_type": "claude", "backing_kind": "pty"});
        assert_eq!(classify_tab_kind(&record), TabKind::Agent);
    }

    #[test]
    fn classify_raw_shell_when_pty_without_agent() {
        let record = serde_json::json!({"id": "t1", "agent_type": null, "backing_kind": "pty"});
        assert_eq!(classify_tab_kind(&record), TabKind::RawShell);
        let empty = serde_json::json!({"id": "t1", "agent_type": "", "backing_kind": "pty"});
        assert_eq!(classify_tab_kind(&empty), TabKind::RawShell);
    }

    #[test]
    fn classify_chat_backend_and_unknown() {
        let chat = serde_json::json!({"id": "t1", "backing_kind": "chat_backend"});
        assert_eq!(classify_tab_kind(&chat), TabKind::ChatBackend);
        let unknown = serde_json::json!({"id": "t1"});
        assert_eq!(classify_tab_kind(&unknown), TabKind::Unknown);
    }

    #[test]
    fn task_shaped_payload_detects_multiline_and_long() {
        assert!(is_task_shaped_payload("first line\nsecond line"));
        assert!(is_task_shaped_payload(&"x".repeat(241)));
        assert!(!is_task_shaped_payload("git status"));
        assert!(!is_task_shaped_payload(&"y".repeat(240)));
    }

    #[test]
    fn agent_tab_allows_prose() {
        assert_eq!(
            decide_send_eligibility(TabKind::Agent, CHARTER, false, false),
            SendEligibility::Allow
        );
    }

    #[test]
    fn raw_shell_refuses_prose_by_default() {
        match decide_send_eligibility(TabKind::RawShell, CHARTER, false, false) {
            SendEligibility::Refuse(msg) => {
                assert!(msg.contains("raw shell"));
                assert!(msg.contains("--to-shell"));
            }
            other => panic!("expected Refuse, got {:?}", other),
        }
    }

    #[test]
    fn raw_shell_allows_short_command() {
        assert_eq!(
            decide_send_eligibility(TabKind::RawShell, "git status", false, false),
            SendEligibility::Allow
        );
    }

    #[test]
    fn to_shell_override_allows_prose_to_shell() {
        assert_eq!(
            decide_send_eligibility(TabKind::RawShell, CHARTER, false, true),
            SendEligibility::Allow
        );
    }

    #[test]
    fn require_agent_refuses_non_agent_targets() {
        for kind in [TabKind::RawShell, TabKind::ChatBackend, TabKind::Unknown] {
            assert!(
                matches!(
                    decide_send_eligibility(kind, "git status", true, false),
                    SendEligibility::Refuse(_)
                ),
                "require_agent must refuse {:?}",
                kind
            );
        }
        assert_eq!(
            decide_send_eligibility(TabKind::Agent, "git status", true, false),
            SendEligibility::Allow
        );
    }

    #[test]
    fn chat_backend_allows_prose_by_default() {
        assert_eq!(
            decide_send_eligibility(TabKind::ChatBackend, CHARTER, false, false),
            SendEligibility::Allow
        );
    }

    #[test]
    fn ok_response_is_success() {
        let v = serde_json::json!({ "ok": true });
        match analyze_send_text_response(&v) {
            SendTextOutcome::Success(_) => {}
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[test]
    fn resolved_text_echo_redacts_secrets() {
        let echo = resolved_text_echo("deliver sk-live-super-secret-token and API_TOKEN=hunter2");
        assert!(echo.starts_with("CODETERM_RESOLVED_TEXT=\"deliver "));
        assert!(echo.contains("***REDACTED***"));
        assert!(!echo.contains("sk-live-super-secret-token"));
        assert!(!echo.contains("hunter2"));
        assert_eq!(
            redact_resolved_send_text("Authorization: Bearer abcdefghijkl"),
            "Authorization: Bearer ***REDACTED***"
        );
    }

    #[test]
    fn explicit_error_field_exits_two() {
        let v = serde_json::json!({
            "ok": false,
            "error": "PTY closed mid-write",
        });
        match analyze_send_text_response(&v) {
            SendTextOutcome::Error { msg, exit_code } => {
                assert_eq!(exit_code, 2);
                assert!(msg.contains("PTY closed"));
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn legitimate_text_passes() {
        assert!(guard_against_shell_mangle("hello world").is_ok());
        assert!(guard_against_shell_mangle("PR #28 — looks fine").is_ok());
        assert!(guard_against_shell_mangle("status code: 200").is_ok());
    }

    #[test]
    fn bash_command_not_found_rejected() {
        let payload = "/usr/bin/bash: line 3: mesh: command not found\nactual content";
        assert!(guard_against_shell_mangle(payload).is_err());
    }

    #[test]
    fn bare_command_not_found_rejected() {
        let payload = "mesh: command not found\nfollowed by intended message";
        assert!(guard_against_shell_mangle(payload).is_err());
    }

    #[test]
    fn syntax_error_rejected() {
        let payload = "bash: -c: line 1: syntax error near unexpected token `('";
        assert!(guard_against_shell_mangle(payload).is_err());
    }

    #[test]
    fn guard_ignores_marker_inside_fenced_code_block() {
        let payload = "Here is the repro:\n```\npunset: command not found\n```\nEnd.";
        assert!(guard_against_shell_mangle(payload).is_ok());
    }

    #[test]
    fn guard_still_fires_on_marker_outside_code_block() {
        let payload = "this is just /usr/bin/bash: line 5: foo not in a code block";
        assert!(guard_against_shell_mangle(payload).is_err());
    }

    #[test]
    fn guard_handles_unterminated_fence_safely() {
        // Unterminated fence treats the rest as code-block — better to skip a potentially-spurious guard than reject legitimate content.
        let payload = "```\n: command not found\nstill open\n";
        assert!(guard_against_shell_mangle(payload).is_ok());
    }

    #[test]
    fn guard_with_multiple_fences_strips_only_inside() {
        let payload = "outside ok\n```\n: command not found\n```\nbetween\n```\nbash: line 1: x\n```\ntrailing";
        assert!(guard_against_shell_mangle(payload).is_ok());
    }

    #[test]
    fn stdin_resolution_preserves_quotes_and_trailing_newline() {
        let input = "it's got 'quotes' and \"doubles\"\nsecond line\n";
        let mut stdin = input.as_bytes();
        let (text, source) = resolve_send_text_input(None, None, None, true, &mut stdin).unwrap();
        assert_eq!(source, "--stdin");
        assert_eq!(text, input);
    }

    #[test]
    fn action_parser_accepts_keys_delays_and_repeats() {
        let parsed = parse_actions_spec("esc,delay:200ms,down*3,enter").unwrap();
        assert_eq!(
            parsed,
            vec![
                SendActionSpec::Key("esc".into()),
                SendActionSpec::Delay(200),
                SendActionSpec::Key("down".into()),
                SendActionSpec::Key("down".into()),
                SendActionSpec::Key("down".into()),
                SendActionSpec::Key("enter".into()),
            ]
        );
    }

    #[test]
    fn action_parser_accepts_ctrl_keys() {
        assert_eq!(key_action_bytes("ctrl-c").unwrap(), vec![3]);
        assert_eq!(
            parse_actions_spec("ctrl-c").unwrap(),
            vec![SendActionSpec::Key("ctrl-c".into())]
        );
    }

    #[test]
    fn actions_parser_accepts_literal_character_keys() {
        assert_eq!(
            parse_actions_spec("y,n,A").unwrap(),
            vec![
                SendActionSpec::Key("y".into()),
                SendActionSpec::Key("n".into()),
                SendActionSpec::Key("A".into()),
            ]
        );
    }

    #[test]
    fn action_parser_rejects_unknown_keys() {
        assert!(parse_actions_spec("launch-missiles").is_err());
    }

    #[test]
    fn composite_tab_id_routes_over_mesh() {
        use crate::cmd::tab::remote_target::parse_remote_tab_id;
        assert_eq!(
            parse_remote_tab_id("mesh:BLACKSTAR/0e39ec9e"),
            Some(("BLACKSTAR".into(), "0e39ec9e".into()))
        );
        assert!(parse_remote_tab_id("0e39ec9e").is_none());
    }

    #[test]
    fn remote_send_body_carries_text_and_source() {
        let body = super::remote_send_body("deliver this", None, Some("af5919cb"));
        assert_eq!(body["text"], serde_json::json!("deliver this"));
        assert_eq!(body["source_tab"], serde_json::json!("af5919cb"));
        assert!(body.get("actions").is_none());
    }

    #[test]
    fn remote_send_body_omits_empty_text_and_blank_source() {
        let actions = parse_actions_spec("enter").unwrap();
        let body = super::remote_send_body("", Some(&actions), Some(""));
        assert!(body.get("text").is_none());
        assert!(body.get("source_tab").is_none());
        assert_eq!(body["actions"], super::actions_to_json(&actions));
    }

    #[test]
    fn pad_nudge_fires_only_for_inlined_authored_payloads() {
        // Short scalar arguments are exactly what may stay inline.
        assert!(super::pad_nudge("positional", "tab_9").is_none());
        assert!(super::pad_nudge("positional", "done").is_none());

        // Multiline is authored prose regardless of length.
        let nudge = super::pad_nudge("positional", "line one\nline two")
            .expect("multiline inline payload is nudged");
        assert!(nudge.contains("--from-pad"));
        assert!(nudge.contains("codeterm pad path"));
        assert!(nudge.contains("multiline"));

        // A long single line is authored too.
        let long = "x".repeat(241);
        assert!(super::pad_nudge("positional", &long)
            .expect("long inline payload is nudged")
            .contains("over 240 characters"));
        assert!(super::pad_nudge("positional", &"y".repeat(240)).is_none());

        // Every non-inline source already did the right thing.
        for source in ["--from-pad", "--stdin", "--from-file"] {
            assert!(
                super::pad_nudge(source, "line one\nline two").is_none(),
                "{source} must not be nudged"
            );
        }
    }

    #[test]
    fn staging_nudge_names_the_caller_supplied_flag() {
        // Same heavy-inline guard: short scalars stay silent.
        assert!(super::staging_nudge("positional", "done", "use --task-file <path>").is_none());
        // Heavy payload surfaces the caller's remedy verbatim, not a hardcoded flag.
        let hint =
            super::staging_nudge("positional", "line one\nline two", "use --task-file <path>")
                .expect("multiline inline payload is nudged");
        assert!(hint.contains("--task-file <path>"));
        assert!(!hint.contains("--from-pad"));
    }
}
