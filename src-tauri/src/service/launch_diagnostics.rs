const TAIL_BYTES: usize = 4096;

#[derive(Clone, Copy)]
pub(crate) enum AgentLaunchOrigin {
    AtomicSpawn,
    ExitRecovery,
    GeneralAgent,
    HiddenSpawn,
    OrchestratedSpawn,
    Restart,
    RestoredSnapshot,
    VisibleSpawn,
}

impl AgentLaunchOrigin {
    fn label(self) -> &'static str {
        match self {
            Self::AtomicSpawn => "atomic_spawn",
            Self::ExitRecovery => "exit_recovery",
            Self::GeneralAgent => "general_agent",
            Self::HiddenSpawn => "hidden_spawn",
            Self::OrchestratedSpawn => "orchestrated_spawn",
            Self::Restart => "restart",
            Self::RestoredSnapshot => "restored_snapshot",
            Self::VisibleSpawn => "visible_spawn",
        }
    }
}

pub(crate) fn log_launch(
    origin: AgentLaunchOrigin,
    tab_id: &str,
    provider: &str,
    command: &str,
    prompt: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    prompt_delivery: &str,
) {
    let prompt_bytes = prompt.map(str::len).unwrap_or(0);
    log::info!(
        "[agent-launch] origin={} tab={} provider={} role={} role_profile={} prompt_delivery={} prompt_present={} prompt_bytes={} prompt_sha256={} command_bytes={} command_sha256={} child_exit=unavailable(pty) stderr=pty",
        origin.label(),
        tab_id,
        provider,
        role.unwrap_or("none"),
        role_profile.unwrap_or("none"),
        prompt_delivery,
        prompt.is_some(),
        prompt_bytes,
        digest(prompt.unwrap_or_default()),
        command.len(),
        digest(command),
    );
}

pub(crate) fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
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
    fn launch_digest_never_contains_source_text() {
        let source = "private launch prompt";
        let got = digest(source);
        assert_eq!(got.len(), 64);
        assert!(!got.contains(source));
    }
}
