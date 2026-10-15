//! Dev-only launch substitution: replay a scripted agent instead of the real
//! provider CLI, keeping provider id, args, detection, and parsing unchanged.

const STUB_CMD_ENV: &str = "CODETERM_STUB_AGENT_CMD";
const STUB_PROVIDERS_ENV: &str = "CODETERM_STUB_AGENT_PROVIDERS";

pub fn is_active() -> bool {
    configured_stub_command().is_some()
}

fn configured_stub_command() -> Option<String> {
    stub_command(cfg!(debug_assertions), std::env::var(STUB_CMD_ENV).ok())
}

fn stub_command(debug_build: bool, raw: Option<String>) -> Option<String> {
    debug_build
        .then_some(raw)
        .flatten()
        .filter(|value| !value.trim().is_empty())
}

/// Whether a launch of `provider_id` would be replayed; `None` outside debug builds.
pub fn provider_stub_replay(provider_id: &str) -> Option<bool> {
    stub_replay_status(
        cfg!(debug_assertions),
        std::env::var(STUB_CMD_ENV).ok(),
        std::env::var(STUB_PROVIDERS_ENV).ok().as_deref(),
        provider_id,
    )
}

fn stub_replay_status(
    debug_build: bool,
    command: Option<String>,
    providers: Option<&str>,
    provider_id: &str,
) -> Option<bool> {
    debug_build
        .then(|| stub_command(true, command).is_some() && provider_enabled(provider_id, providers))
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
    fn stub_replay_status_is_debug_only_and_per_provider() {
        let cmd = || Some("node replay.mjs".to_string());
        assert_eq!(stub_replay_status(false, cmd(), None, "alpha"), None);
        assert_eq!(stub_replay_status(true, None, None, "alpha"), Some(false));
        assert_eq!(
            stub_replay_status(true, Some("  ".into()), None, "alpha"),
            Some(false)
        );
        assert_eq!(stub_replay_status(true, cmd(), None, "alpha"), Some(true));
        assert_eq!(
            stub_replay_status(true, cmd(), Some(""), "beta"),
            Some(true)
        );
        assert_eq!(
            stub_replay_status(true, cmd(), Some("beta, Alpha"), "alpha"),
            Some(true)
        );
        assert_eq!(
            stub_replay_status(true, cmd(), Some("beta"), "alpha"),
            Some(false)
        );
    }

    #[test]
    fn gate_and_wrap_behavior() {
        assert_eq!(
            apply_configured(
                "alpha",
                "alpha --model opus".into(),
                "node replay.mjs",
                Some("beta")
            ),
            "alpha --model opus"
        );

        assert_eq!(
            apply_configured(
                "alpha",
                "alpha --model opus".into(),
                "node replay.mjs",
                None
            ),
            "node replay.mjs --stub-provider alpha -- alpha --model opus"
        );
        assert_eq!(
            apply_configured("beta", "beta exec".into(), "node replay.mjs", Some("beta")),
            "node replay.mjs --stub-provider beta -- beta exec"
        );
        let compound = apply_configured(
            "alpha",
            "Remove-Item Env:X; $A = 1; alpha '--model' 'sonnet'".into(),
            "node replay.mjs",
            None,
        );
        assert_eq!(
            compound,
            "Remove-Item Env:X; $A = 1; node replay.mjs --stub-provider alpha -- alpha '--model' 'sonnet'"
        );
        let posix_prep = apply_configured(
            "alpha",
            "unset CLAUDECODE && alpha --dangerously-skip-permissions '--model' 'sonnet' -- \"$(cat /tmp/.ct-task-1234)\"".into(),
            "node replay.mjs",
            None,
        );
        assert_eq!(
            posix_prep,
            "unset CLAUDECODE && node replay.mjs --stub-provider alpha -- alpha --dangerously-skip-permissions '--model' 'sonnet' -- \"$(cat /tmp/.ct-task-1234)\""
        );
    }
}
