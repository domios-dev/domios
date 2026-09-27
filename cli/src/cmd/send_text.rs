use crate::client::SocketClient;
use crate::cmd::print::print_human;
use crate::cmd::resolved_text::resolved_text_echo;
use crate::cmd::tab::remote_target::parse_remote_tab_id;
use crate::resolve_tab;

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
    Pending {
        message: String,
        bytes_written: bool,
    },
    Error {
        msg: String,
        exit_code: i32,
    },
    Other,
}

pub fn analyze_send_text_response(value: &serde_json::Value) -> SendTextOutcome {
    if let Some(err) = value.get("error") {
        if err
            .get("bytes_written")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return SendTextOutcome::Pending {
                message: "OK (pending)".to_string(),
                bytes_written: true,
            };
        }
        let message = err
            .as_str()
            .or_else(|| err.get("message").and_then(serde_json::Value::as_str))
            .or_else(|| err.get("code").and_then(serde_json::Value::as_str))
            .unwrap_or("send was refused before the payload was written");
        return SendTextOutcome::Error {
            msg: message.to_string(),
            exit_code: 2,
        };
    }
    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return match value.get("delivery").and_then(serde_json::Value::as_str) {
            Some("sent") => SendTextOutcome::Success("OK (sent)".to_string()),
            _ => SendTextOutcome::Pending {
                message: "OK (pending)".to_string(),
                bytes_written: value
                    .get("bytes_written")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            },
        };
    }
    SendTextOutcome::Other
}

fn pad_file_path(value: &serde_json::Value) -> Option<&str> {
    value.get("pad_file").and_then(serde_json::Value::as_str)
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
            let pad_file = pad_file_path(&value);
            match analyze_send_text_response(&value) {
                SendTextOutcome::Success(msg) => {
                    println!("{}", msg);
                    if let Some(path) = pad_file {
                        println!("Pad file: {path}");
                    }
                    if let Some(text) = resolved_text {
                        println!("{}", resolved_text_echo(text));
                    }
                }
                SendTextOutcome::Pending {
                    message,
                    bytes_written,
                } => {
                    println!("{}", message);
                    if let Some(path) = pad_file {
                        println!("Pad file: {path}");
                    }
                    if bytes_written {
                        if let Some(text) = resolved_text {
                            println!("{}", resolved_text_echo(text));
                        }
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
    crate::cmd::quoted_handoff::is_authored_payload(text)
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
/// Split a `--tab` value into targets: comma-separated, trimmed, deduped in
/// first-seen order. A value without a comma yields exactly one target, so the
/// single-tab path stays untouched.
pub fn parse_tab_targets(spec: &str) -> Result<Vec<String>, String> {
    let mut targets: Vec<String> = Vec::new();
    for raw in spec.split(',') {
        let id = raw.trim();
        if id.is_empty() {
            return Err(format!(
                "--tab has an empty entry in '{spec}': list ids as a,b,c without blanks"
            ));
        }
        if !targets.iter().any(|seen| seen == id) {
            targets.push(id.to_string());
        }
    }
    Ok(targets)
}

/// Deliver one already-resolved payload to several tabs: the single-tab call
/// repeated per target, never a new delivery route. Reports every target and
/// fails loudly if any of them did.
fn send_text_to_targets(
    client: &mut SocketClient,
    json: bool,
    targets: &[String],
    resolved_text: &str,
    actions: Option<Vec<SendActionSpec>>,
    require_agent: bool,
    to_shell: bool,
) {
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut delivered: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut pending_written: Vec<String> = Vec::new();
    let mut pad_files: Vec<serde_json::Value> = Vec::new();
    let source_tab = std::env::var("CODETERM_TAB_ID")
        .ok()
        .filter(|t| !t.is_empty());
    for tab_id in targets {
        if !resolved_text.trim().is_empty() {
            let kind = probe_tab_kind(client, tab_id);
            if let SendEligibility::Refuse(msg) =
                decide_send_eligibility(kind, resolved_text, require_agent, to_shell)
            {
                failures.push((tab_id.clone(), msg));
                continue;
            }
        }
        let result = match actions.clone() {
            Some(actions) => send_actions_via_socket(json, tab_id, resolved_text, actions),
            None => {
                let mut params = serde_json::json!({
                    "tab_id": tab_id,
                    "text": resolved_text,
                });
                if let Some(ref my_tab) = source_tab {
                    if my_tab != tab_id {
                        params["source_tab"] = serde_json::json!(my_tab);
                    }
                }
                client.call("tab.send_text", Some(params))
            }
        };
        match result {
            Ok(value) => {
                if let Some(path) = pad_file_path(&value) {
                    pad_files.push(serde_json::json!({"tab": tab_id, "path": path}));
                }
                match analyze_send_text_response(&value) {
                    SendTextOutcome::Success(_) => delivered.push(tab_id.clone()),
                    SendTextOutcome::Pending { bytes_written, .. } => {
                        pending.push(tab_id.clone());
                        if bytes_written {
                            pending_written.push(tab_id.clone());
                        }
                    }
                    SendTextOutcome::Error { msg, .. } => failures.push((tab_id.clone(), msg)),
                    SendTextOutcome::Other => failures.push((
                        tab_id.clone(),
                        "send route returned no delivery status".into(),
                    )),
                }
            }
            Err(e) => failures.push((tab_id.clone(), e)),
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": failures.is_empty(),
                "delivered": delivered,
                "pending": pending,
                "pending_written": pending_written,
                "pad_files": pad_files,
                "failed": failures
                    .iter()
                    .map(|(tab, err)| serde_json::json!({ "tab": tab, "error": err }))
                    .collect::<Vec<_>>(),
            }))
            .unwrap()
        );
    } else {
        for tab_id in &delivered {
            println!("OK (sent) {tab_id}");
        }
        for tab_id in &pending {
            println!("OK (pending) {tab_id}");
        }
        for entry in &pad_files {
            if let (Some(tab), Some(path)) = (entry["tab"].as_str(), entry["path"].as_str()) {
                println!("Pad file {tab}: {path}");
            }
        }
        for (tab_id, err) in &failures {
            eprintln!("Error {tab_id}: {err}");
        }
        println!(
            "{} sent, {} pending, {} refused of {} target(s)",
            delivered.len(),
            pending.len(),
            failures.len(),
            targets.len()
        );
        if !delivered.is_empty() || !pending_written.is_empty() {
            println!("{}", resolved_text_echo(resolved_text));
        }
    }
    if !failures.is_empty() {
        std::process::exit(2);
    }
}

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
    every: Option<String>,
    until: Option<String>,
    count: Option<u32>,
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
            every,
            until,
            count,
        );
        return;
    }
    if delay.is_some() || at.is_some() || every.is_some() || until.is_some() || count.is_some() {
        if actions.is_some() {
            eprintln!("Error: --actions cannot be queued with --delay/--at/--every");
            std::process::exit(2);
        }
        crate::cmd::delayed::queue_from_send(
            client, json, tab, text, from_file, from_pad, from_stdin, delay, at, every, until,
            count,
        );
        return;
    }
    let multi_targets = match tab.as_deref() {
        Some(spec) if spec.contains(',') => match parse_tab_targets(spec) {
            Ok(targets) => {
                if let Some(composite) = targets.iter().find(|t| parse_remote_tab_id(t).is_some()) {
                    emit_send_refusal(
                        json,
                        &format!(
                            "--tab list contains the mesh target '{composite}': run multi-tab send on the owning peer, one peer per call"
                        ),
                    );
                }
                Some(targets)
            }
            Err(msg) => emit_send_refusal(json, &msg),
        },
        _ => None,
    };
    let mut stdin = std::io::stdin();
    let (resolved_text, parsed_actions) =
        resolve_and_validate_send(text, from_file, from_pad, from_stdin, actions, &mut stdin);
    if let Some(targets) = multi_targets {
        send_text_to_targets(
            client,
            json,
            &targets,
            &resolved_text,
            parsed_actions,
            require_agent,
            to_shell,
        );
        return;
    }
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
    every: Option<String>,
    until: Option<String>,
    count: Option<u32>,
) {
    let mut stdin = std::io::stdin();
    let (resolved_text, parsed_actions) =
        resolve_and_validate_send(text, from_file, from_pad, from_stdin, actions, &mut stdin);
    if delay.is_some() || at.is_some() || every.is_some() || until.is_some() || count.is_some() {
        if parsed_actions.is_some() {
            eprintln!("Error: --actions cannot be queued with --delay/--at/--every");
            std::process::exit(2);
        }
        crate::cmd::delayed::queue_over_mesh(
            json,
            peer,
            remote_tab,
            resolved_text,
            delay,
            at,
            every,
            until,
            count,
        );
        return;
    }
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
    if let Some(refusal) = crate::cmd::quoted_handoff::inline_refusal(
        crate::cmd::quoted_handoff::INLINE_SOURCE,
        &text,
        crate::cmd::quoted_handoff::PAD_REMEDY,
    ) {
        return Err(refusal);
    }
    Ok((text, crate::cmd::quoted_handoff::INLINE_SOURCE))
}

#[cfg(test)]
mod tests;
