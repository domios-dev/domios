use crate::client::SocketClient;
use crate::cmd::print::print_human;
use crate::resolve_pane;

/// Detect content that's almost certainly the result of bash command-substitution
/// eating the agent's intended text — e.g. unquoted `mesh peers` inside a
/// double-quoted argv string causes bash to execute `mesh peers` and splice the
/// resulting "command not found" stderr into the payload before send-text sees
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
                "send-text payload looks shell-mangled (matched marker: {:?})",
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
        return SendTextOutcome::Error { msg: err.to_string(), exit_code: 2 };
    }
    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return SendTextOutcome::Success("OK (sent)".to_string());
    }
    SendTextOutcome::Other
}

pub fn handle_send_text_result(json: bool, result: Result<serde_json::Value, String>) {
    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                return;
            }
            match analyze_send_text_response(&value) {
                SendTextOutcome::Success(msg) => println!("{}", msg),
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

pub fn handle_send_text(
    client: &mut SocketClient,
    json: bool,
    pane: Option<String>,
    text: String,
    from_file: Option<String>,
    from_stdin: bool,
) {
    let (resolved_text, source) = if from_stdin {
        use std::io::Read;
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
            eprintln!("Error: failed to read --from-stdin: {}", e);
            std::process::exit(1);
        }
        (buf, "--from-stdin")
    } else if let Some(path) = from_file {
        match std::fs::read_to_string(&path) {
            Ok(s) => (s, "--from-file"),
            Err(e) => {
                eprintln!("Error: failed to read --from-file {}: {}", path, e);
                std::process::exit(1);
            }
        }
    } else {
        (text, "positional")
    };
    // Reject empty payloads loudly. Silent zero-byte sends were the bug behind
    // several "mesh sent but recipient saw empty" reports.
    if resolved_text.trim().is_empty() {
        eprintln!("Error: send-text payload is empty (source: {})", source);
        eprintln!("hint: if using --from-stdin, ensure the heredoc actually delivered content;");
        eprintln!("      if --from-file, verify the path exists and isn't 0 bytes;");
        eprintln!("      if positional, your shell may have eaten the content.");
        std::process::exit(2);
    }
    if let Err(msg) = guard_against_shell_mangle(&resolved_text) {
        eprintln!("Error: {}", msg);
        eprintln!("hint: your shell expanded a command-substitution before codeterm-CLI received it.");
        eprintln!("      Use --from-file <path> or --from-stdin <<'EOF' ... EOF for content with backticks/$/code.");
        std::process::exit(2);
    }
    let pane_id = resolve_pane(client, pane);
    let mut params = serde_json::json!({
        "pane_id": pane_id,
        "text": resolved_text,
    });
    // Carry the sender's pane id when running inside CodeTerm so the daemon
    // can prepend a [from <title> pane=<id>] return-address prefix on the
    // recipient side. Skip when target is self (chatting with own pane —
    // no point self-tagging).
    if let Ok(my_pane) = std::env::var("CODETERM_PANE_ID") {
        if !my_pane.is_empty() && my_pane != pane_id {
            params["source_pane"] = serde_json::json!(my_pane);
        }
    }
    let result = client.call("pane.send_text", Some(params));
    handle_send_text_result(json, result);
}

#[cfg(test)]
mod tests {
    use super::{analyze_send_text_response, guard_against_shell_mangle, SendTextOutcome};

    #[test]
    fn ok_response_is_success() {
        let v = serde_json::json!({ "ok": true });
        match analyze_send_text_response(&v) {
            SendTextOutcome::Success(_) => {}
            other => panic!("expected Success, got {:?}", other),
        }
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
        // Unterminated fence treats the rest as code-block — better to skip a
        // potentially-spurious guard than reject legitimate content.
        let payload = "```\n: command not found\nstill open\n";
        assert!(guard_against_shell_mangle(payload).is_ok());
    }

    #[test]
    fn guard_with_multiple_fences_strips_only_inside() {
        let payload = "outside ok\n```\n: command not found\n```\nbetween\n```\nbash: line 1: x\n```\ntrailing";
        assert!(guard_against_shell_mangle(payload).is_ok());
    }
}
