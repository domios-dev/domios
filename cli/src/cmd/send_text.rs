use crate::client::SocketClient;
use crate::cmd::print::print_human;
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
    from_stdin: bool,
    actions: Option<String>,
    delay: Option<String>,
    at: Option<String>,
) {
    if delay.is_some() || at.is_some() {
        if actions.is_some() {
            eprintln!("Error: --actions cannot be queued with --delay/--at");
            std::process::exit(2);
        }
        crate::cmd::delayed::queue_from_send(
            client, json, tab, text, from_file, from_stdin, delay, at,
        );
        return;
    }
    let mut stdin = std::io::stdin();
    let (resolved_text, source) =
        match resolve_send_text_input(text, from_file, from_stdin, &mut stdin) {
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
    let tab_id = resolve_tab(client, tab);
    if let Some(actions) = parsed_actions {
        let result = send_actions_via_http(json, &tab_id, &resolved_text, actions);
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
    let lower = token.trim().to_ascii_lowercase();
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
    let key = canonical_key_name(&lower)?;
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
    let mapped = match normalized.as_str() {
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
        _ => {
            return Err(format!(
                "unknown action key '{}'; use enter, esc, tab, backspace, delete, arrows, home/end, pageup/pagedown, space, ctrl-c",
                key
            ))
        }
    };
    Ok(mapped.to_string())
}

#[cfg(test)]
fn key_action_bytes(key: &str) -> Option<Vec<u8>> {
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

fn send_actions_via_http(
    _json: bool,
    tab_id: &str,
    text: &str,
    actions: Vec<SendActionSpec>,
) -> Result<serde_json::Value, String> {
    let (base, token) = crate::read_api_config();
    let http = crate::build_http(&token).map_err(|e| e.to_string())?;
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
    let resp = http
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
    from_stdin: bool,
    stdin: &mut R,
) -> Result<(String, &'static str), String> {
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
    Ok((text.unwrap_or_default(), "positional"))
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_send_text_response, guard_against_shell_mangle, key_action_bytes,
        parse_actions_spec, redact_resolved_send_text, resolve_send_text_input, resolved_text_echo,
        SendActionSpec, SendTextOutcome,
    };

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
        let (text, source) = resolve_send_text_input(None, None, true, &mut stdin).unwrap();
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
    fn action_parser_rejects_unknown_keys() {
        assert!(parse_actions_spec("launch-missiles").is_err());
    }
}
