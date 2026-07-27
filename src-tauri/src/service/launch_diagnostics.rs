const TAIL_BYTES: usize = 4096;

pub(crate) fn log_launch(
    context: &str,
    tab_id: &str,
    provider: &str,
    command: &str,
    task: Option<&str>,
) {
    let task_bytes = task.map(|t| t.len()).unwrap_or(0);
    let transport = task_transport(command);
    log::info!(
        "[launch-diagnostics] context={} tab={} provider={} command_bytes={} task_bytes={} task_transport={} child_exit=unavailable(pty) stderr=pty command={}",
        context,
        tab_id,
        provider,
        command.len(),
        task_bytes,
        transport,
        redact_secrets(command),
    );
}

pub(crate) fn log_shell_drop(
    tab_id: &str,
    pty_id: &str,
    output: &str,
    reason: &str,
    restarts: u32,
    max_restarts: u32,
) {
    let tail = tail_bytes(&crate::pty::ansi::clean_terminal_output(output), TAIL_BYTES);
    log::warn!(
        "[launch-diagnostics] shell-drop tab={} pty={} reason={} restarts={}/{} child_exit=unavailable(pty) stderr=pty last_output_bytes={} last_output_tail={:?}",
        tab_id,
        pty_id,
        reason,
        restarts,
        max_restarts,
        tail.len(),
        redact_secrets(&tail),
    );
}

fn task_transport(command: &str) -> &'static str {
    if command.contains("--append-system-prompt-file") {
        "file_arg"
    } else if command.contains("Get-Content") || command.contains("$(cat ") {
        "shell_substitution_argv"
    } else if command.contains(" | claude") || command.contains("| claude") {
        "stdin_pipe"
    } else {
        "none_or_unknown"
    }
}

pub(crate) fn tail_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut start = text.len() - max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

pub(crate) fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for key in [
        "CODETERM_API_TOKEN",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "Authorization: Bearer",
    ] {
        out = redact_after_marker(&out, key);
    }
    out
}

fn redact_after_marker(input: &str, marker: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(marker) {
        out.push_str(&rest[..idx + marker.len()]);
        let after = &rest[idx + marker.len()..];
        let (redacted_prefix, consumed) = redact_value_prefix(after);
        out.push_str(&redacted_prefix);
        rest = &after[consumed..];
    }
    out.push_str(rest);
    out
}

fn redact_value_prefix(input: &str) -> (String, usize) {
    let mut consumed = 0usize;
    let mut prefix = String::new();
    for ch in input.chars() {
        if ch == '=' || ch.is_whitespace() || ch == '"' || ch == '\'' {
            prefix.push(ch);
            consumed += ch.len_utf8();
            continue;
        }
        break;
    }
    let value_start = consumed;
    for ch in input[value_start..].chars() {
        if ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ';' {
            break;
        }
        consumed += ch.len_utf8();
    }
    if consumed > value_start {
        prefix.push_str("<redacted>");
    }
    (prefix, consumed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secret_markers() {
        let input = "CODETERM_API_TOKEN=abc123; curl -H \"Authorization: Bearer xyz789\"";
        let got = redact_secrets(input);
        assert!(!got.contains("abc123"));
        assert!(!got.contains("xyz789"));
        assert!(got.contains("CODETERM_API_TOKEN=<redacted>"));
        assert!(got.contains("Authorization: Bearer <redacted>"));
    }

    #[test]
    fn tail_bytes_preserves_utf8_boundary() {
        let input = format!("{}end", "привет".repeat(100));
        let got = tail_bytes(&input, 17);
        assert!(got.ends_with("end"));
        assert!(got.is_char_boundary(0));
    }

    #[test]
    fn task_transport_classifies_inline_substitution() {
        assert_eq!(
            task_transport("claude -- \"$(Get-Content 'x' -Raw -Encoding UTF8)\""),
            "shell_substitution_argv"
        );
        assert_eq!(
            task_transport("claude --append-system-prompt-file 'x' -- 'run'"),
            "file_arg"
        );
    }
}
