//! Dev-only launch substitution: replay a scripted agent instead of the real
//! provider CLI, keeping provider id, args, detection, and parsing unchanged.

const STUB_CMD_ENV: &str = "CODETERM_STUB_AGENT_CMD";
const STUB_PROVIDERS_ENV: &str = "CODETERM_STUB_AGENT_PROVIDERS";

pub fn is_active() -> bool {
    cfg!(debug_assertions) && std::env::var(STUB_CMD_ENV).map_or(false, |v| !v.trim().is_empty())
}

fn provider_enabled(provider_id: &str) -> bool {
    match std::env::var(STUB_PROVIDERS_ENV) {
        Ok(list) if !list.trim().is_empty() => list
            .split(',')
            .any(|p| p.trim().eq_ignore_ascii_case(provider_id)),
        _ => true,
    }
}

/// Wrap the provider invocation in the stub replayer when the dev gate is on.
/// Launch commands can be multi-statement shell lines (Windows PowerShell env
/// prep before the CLI); only the FINAL statement is the provider argv, so the
/// prep statements are kept and the last one is wrapped. The original argv is
/// forwarded verbatim after `--` so the replayer mirrors model/resume flags.
pub fn apply(provider_id: &str, cmd: String) -> String {
    if !is_active() || !provider_enabled(provider_id) {
        return cmd;
    }
    let stub = std::env::var(STUB_CMD_ENV).unwrap_or_default();
    log::info!(
        "[stub-agent] substituting launch for provider={}",
        provider_id
    );
    match split_prep_and_invocation(&cmd) {
        Some((prep, sep, invocation)) => format!(
            "{}{} {} --stub-provider {} -- {}",
            prep,
            sep,
            stub,
            provider_id,
            invocation.trim()
        ),
        None => format!("{} --stub-provider {} -- {}", stub, provider_id, cmd),
    }
}

fn split_prep_and_invocation(cmd: &str) -> Option<(&str, &str, &str)> {
    let semicolon = cmd.rmatch_indices(';').next().map(|(i, _)| (i, ";"));
    let and = cmd.rmatch_indices(" && ").next().map(|(i, _)| (i, " && "));
    let (idx, sep) = match (semicolon, and) {
        (Some(a), Some(b)) => {
            if a.0 > b.0 {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    let invocation_start = idx + sep.len();
    Some((&cmd[..idx], sep, &cmd[invocation_start..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Single test: env vars are process-global and cargo runs tests in parallel.
    #[test]
    fn gate_and_wrap_behavior() {
        std::env::remove_var(STUB_CMD_ENV);
        std::env::remove_var(STUB_PROVIDERS_ENV);
        assert_eq!(
            apply("claude", "claude --model opus".into()),
            "claude --model opus"
        );

        if cfg!(debug_assertions) {
            std::env::set_var(STUB_CMD_ENV, "node replay.mjs");
            assert_eq!(
                apply("claude", "claude --model opus".into()),
                "node replay.mjs --stub-provider claude -- claude --model opus"
            );
            std::env::set_var(STUB_PROVIDERS_ENV, "codex");
            assert_eq!(
                apply("claude", "claude --model opus".into()),
                "claude --model opus"
            );
            assert_eq!(
                apply("codex", "codex exec".into()),
                "node replay.mjs --stub-provider codex -- codex exec"
            );
            std::env::remove_var(STUB_PROVIDERS_ENV);
            let compound = apply(
                "claude",
                "Remove-Item Env:X; $A = 1; claude '--model' 'sonnet'".into(),
            );
            assert_eq!(
                compound,
                "Remove-Item Env:X; $A = 1; node replay.mjs --stub-provider claude -- claude '--model' 'sonnet'"
            );
            let posix_prep = apply(
                "claude",
                "unset CLAUDECODE && claude --dangerously-skip-permissions '--model' 'sonnet' -- \"$(cat /tmp/.ct-task-1234)\"".into(),
            );
            assert_eq!(
                posix_prep,
                "unset CLAUDECODE && node replay.mjs --stub-provider claude -- claude --dangerously-skip-permissions '--model' 'sonnet' -- \"$(cat /tmp/.ct-task-1234)\""
            );
            std::env::remove_var(STUB_CMD_ENV);
        }
    }
}
