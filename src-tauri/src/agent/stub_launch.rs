//! Dev-only launch substitution: replay a scripted agent instead of the real
//! provider CLI, keeping provider id, args, detection, and parsing unchanged.

const STUB_CMD_ENV: &str = "CODETERM_STUB_AGENT_CMD";
const STUB_PROVIDERS_ENV: &str = "CODETERM_STUB_AGENT_PROVIDERS";

pub fn is_active() -> bool {
    configured_stub_command().is_some()
}

fn configured_stub_command() -> Option<String> {
    cfg!(debug_assertions)
        .then(|| std::env::var(STUB_CMD_ENV).ok())
        .flatten()
        .filter(|value| !value.trim().is_empty())
}

fn provider_enabled(provider_id: &str, configured: Option<&str>) -> bool {
    match configured {
        Some(list) if !list.trim().is_empty() => list
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
    let Some(stub) = configured_stub_command() else {
        return cmd;
    };
    let providers = std::env::var(STUB_PROVIDERS_ENV).ok();
    apply_configured(provider_id, cmd, &stub, providers.as_deref())
}

fn apply_configured(provider_id: &str, cmd: String, stub: &str, providers: Option<&str>) -> String {
    if !provider_enabled(provider_id, providers) {
        return cmd;
    }
    log::info!(
        "[stub-agent] substituting launch for provider={}",
        provider_id
    );
    match split_prep_and_invocation(&cmd) {
        Some((prep, sep, invocation)) => {
            let space = if sep.ends_with(' ') { "" } else { " " };
            format!(
                "{}{}{}{} --stub-provider {} -- {}",
                prep,
                sep,
                space,
                stub,
                provider_id,
                invocation.trim()
            )
        }
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

    #[test]
    fn gate_and_wrap_behavior() {
        assert_eq!(
            apply_configured(
                "claude",
                "claude --model opus".into(),
                "node replay.mjs",
                Some("codex")
            ),
            "claude --model opus"
        );

        assert_eq!(
            apply_configured(
                "claude",
                "claude --model opus".into(),
                "node replay.mjs",
                None
            ),
            "node replay.mjs --stub-provider claude -- claude --model opus"
        );
        assert_eq!(
            apply_configured(
                "codex",
                "codex exec".into(),
                "node replay.mjs",
                Some("codex")
            ),
            "node replay.mjs --stub-provider codex -- codex exec"
        );
        let compound = apply_configured(
            "claude",
            "Remove-Item Env:X; $A = 1; claude '--model' 'sonnet'".into(),
            "node replay.mjs",
            None,
        );
        assert_eq!(
            compound,
            "Remove-Item Env:X; $A = 1; node replay.mjs --stub-provider claude -- claude '--model' 'sonnet'"
        );
        let posix_prep = apply_configured(
            "claude",
            "unset CLAUDECODE && claude --dangerously-skip-permissions '--model' 'sonnet' -- \"$(cat /tmp/.ct-task-1234)\"".into(),
            "node replay.mjs",
            None,
        );
        assert_eq!(
            posix_prep,
            "unset CLAUDECODE && node replay.mjs --stub-provider claude -- claude --dangerously-skip-permissions '--model' 'sonnet' -- \"$(cat /tmp/.ct-task-1234)\""
        );
    }
}
