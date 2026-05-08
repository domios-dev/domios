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

pub fn handle_send_text_result(json: bool, result: Result<serde_json::Value, String>) {
    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                return;
            }
            if let Some(verified) = value.get("verified").and_then(|v| v.as_bool()) {
                if verified {
                    let elapsed = value.get("elapsed_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    let echo = value.get("echo_observed").and_then(|v| v.as_bool()).unwrap_or(false);
                    let submit = value.get("submit_observed").and_then(|v| v.as_bool()).unwrap_or(false);
                    println!("OK (verified in {}ms; echo={}, submit={})", elapsed, echo, submit);
                } else {
                    let err = value.get("error").and_then(|v| v.as_str()).unwrap_or("verification failed");
                    eprintln!("Error: {}", err);
                    std::process::exit(2);
                }
                return;
            }
            print_human(&value);
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
    no_wait: bool,
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
    if no_wait {
        params["verify"] = serde_json::json!(false);
    }
    let result = client.call("pane.send_text", Some(params));
    handle_send_text_result(json, result);
}

#[cfg(test)]
mod tests {
    use super::guard_against_shell_mangle;

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
