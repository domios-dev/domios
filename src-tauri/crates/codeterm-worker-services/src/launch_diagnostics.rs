const TAIL_BYTES: usize = 4096;

mod account_home_diagnostic;
mod failure_screen;
pub use failure_screen::log_failure_screen;

#[derive(Debug, PartialEq, Eq)]
struct LaunchEnvironmentSummary {
    api_target: String,
    api_target_source: &'static str,
    tab_id_present: bool,
    tab_id_matches: bool,
    token_present: bool,
    token_source: &'static str,
    binding_nonce_present: bool,
    binding_nonce_source: &'static str,
    codex_home_present: bool,
    codex_home_source: &'static str,
    path_system32_present: bool,
    comspec_present: bool,
    path_entry_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLaunchOrigin {
    AtomicSpawn,
    AutomaticExitRecovery,
    ExitRecovery,
    GeneralAgent,
    HiddenSpawn,
    OrchestratedSpawn,
    Restart,
    Resume,
    RestoredSnapshot,
    VisibleSpawn,
}

impl AgentLaunchOrigin {
    /// Relaunches a tab whose agent was already dispatched; no startup waiter is owed.
    pub fn relaunches_existing_agent(self) -> bool {
        matches!(
            self,
            Self::AutomaticExitRecovery
                | Self::ExitRecovery
                | Self::Restart
                | Self::Resume
                | Self::RestoredSnapshot
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::AtomicSpawn => "atomic_spawn",
            Self::AutomaticExitRecovery => "automatic_exit_recovery",
            Self::ExitRecovery => "exit_recovery",
            Self::GeneralAgent => "general_agent",
            Self::HiddenSpawn => "hidden_spawn",
            Self::OrchestratedSpawn => "orchestrated_spawn",
            Self::Restart => "restart",
            Self::Resume => "resume",
            Self::RestoredSnapshot => "restored_snapshot",
            Self::VisibleSpawn => "visible_spawn",
        }
    }
}

pub fn log_launch_environment(
    origin: &str,
    tab_id: &str,
    launch_env: &std::collections::HashMap<String, String>,
    spawn_blocking: impl FnOnce(Box<dyn FnOnce() + Send>),
) {
    let inherited_env = [
        "CODETERM_API_URL",
        "CODETERM_TAB_ID",
        "CODETERM_API_TOKEN",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "PATH",
        "ComSpec",
    ]
    .into_iter()
    .filter_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| (name.to_string(), value))
    })
    .collect();
    account_home_diagnostic::schedule(tab_id, launch_env, &inherited_env, spawn_blocking);
    let cli_path = launch_env
        .get("CODETERM_CLI_PATH")
        .map(|path| std::ffi::OsString::from(path.as_str()))
        .or_else(|| std::env::var_os("CODETERM_CLI_PATH"));
    let cli_dir = cli_path
        .as_deref()
        .and_then(codeterm_pty::pty::manager::cli_dir_of);
    let summary =
        summarize_launch_environment(tab_id, launch_env, &inherited_env, cli_dir.as_deref());
    log::info!(
        "[agent-launch-env] origin={} tab={} api_target={} api_target_source={} tab_id_present={} tab_id_matches={} api_token_present={} api_token_source={} binding_nonce_present={} binding_nonce_source={} codex_home_present={} codex_home_source={} path_system32_present={} comspec_present={} path_entry_count={}",
        origin,
        tab_id,
        summary.api_target,
        summary.api_target_source,
        summary.tab_id_present,
        summary.tab_id_matches,
        summary.token_present,
        summary.token_source,
        summary.binding_nonce_present,
        summary.binding_nonce_source,
        summary.codex_home_present,
        summary.codex_home_source,
        summary.path_system32_present,
        summary.comspec_present,
        summary.path_entry_count,
    );
}

fn summarize_launch_environment(
    tab_id: &str,
    launch_env: &std::collections::HashMap<String, String>,
    inherited_env: &std::collections::HashMap<String, String>,
    cli_dir: Option<&std::path::Path>,
) -> LaunchEnvironmentSummary {
    let (api_url, api_target_source) =
        effective_env_value(launch_env, inherited_env, "CODETERM_API_URL");
    let (api_token, token_source) =
        effective_env_value(launch_env, inherited_env, "CODETERM_API_TOKEN");
    let (binding_nonce, binding_nonce_source) =
        effective_env_value(launch_env, inherited_env, "CODETERM_SESSION_BINDING_NONCE");
    let (launch_tab_id, _) = effective_env_value(launch_env, inherited_env, "CODETERM_TAB_ID");
    let (codex_home, codex_home_source) =
        effective_env_value(launch_env, inherited_env, "CODEX_HOME");
    let (path, _) = effective_env_value_case_insensitive(launch_env, inherited_env, "PATH");
    let (comspec, _) = effective_env_value_case_insensitive(launch_env, inherited_env, "ComSpec");
    let mut path_entries = path
        .as_deref()
        .map(|value| std::env::split_paths(value))
        .into_iter()
        .flatten()
        .filter(|entry| !entry.as_os_str().is_empty())
        .collect::<Vec<_>>();
    if let Some(cli_dir) = cli_dir {
        path_entries.retain(|entry| entry != cli_dir);
        path_entries.insert(0, cli_dir.to_path_buf());
    }
    LaunchEnvironmentSummary {
        api_target: api_target(api_url.as_deref()),
        api_target_source,
        tab_id_present: launch_tab_id.is_some(),
        tab_id_matches: launch_tab_id.as_deref() == Some(tab_id),
        token_present: api_token.is_some(),
        token_source,
        binding_nonce_present: binding_nonce.is_some(),
        binding_nonce_source,
        codex_home_present: codex_home.is_some(),
        codex_home_source,
        path_system32_present: path_entries.iter().any(is_system32_path_entry),
        comspec_present: comspec.is_some(),
        path_entry_count: path_entries.len(),
    }
}

fn is_system32_path_entry(entry: &std::path::PathBuf) -> bool {
    let entry = entry.as_os_str().to_string_lossy();
    std::path::Path::new(entry.trim_matches('"'))
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("System32"))
}

fn effective_env_value_case_insensitive(
    launch_env: &std::collections::HashMap<String, String>,
    inherited_env: &std::collections::HashMap<String, String>,
    name: &str,
) -> (Option<String>, &'static str) {
    let launch_value = case_insensitive_env_value(launch_env, name);
    if let Some(value) = launch_value {
        return ((!value.trim().is_empty()).then(|| value.clone()), "launch");
    }
    let inherited_value = case_insensitive_env_value(inherited_env, name);
    match inherited_value {
        Some(value) => (
            (!value.trim().is_empty()).then(|| value.clone()),
            "inherited",
        ),
        None => (None, "unset"),
    }
}

fn case_insensitive_env_value<'a>(
    env: &'a std::collections::HashMap<String, String>,
    name: &str,
) -> Option<&'a String> {
    env.get(name).or_else(|| {
        env.iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .min_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(_, value)| value)
    })
}

fn effective_env_value(
    launch_env: &std::collections::HashMap<String, String>,
    inherited_env: &std::collections::HashMap<String, String>,
    name: &str,
) -> (Option<String>, &'static str) {
    if let Some(value) = launch_env.get(name) {
        return ((!value.trim().is_empty()).then(|| value.clone()), "launch");
    }
    match inherited_env.get(name) {
        Some(value) => (
            (!value.trim().is_empty()).then(|| value.clone()),
            "inherited",
        ),
        None => (None, "unset"),
    }
}

fn api_target(api_url: Option<&str>) -> String {
    let Some(api_url) = api_url else {
        return "unset".into();
    };
    let Ok(parsed) = url::Url::parse(api_url) else {
        return "invalid".into();
    };
    let (Some(host), Some(port)) = (parsed.host_str(), parsed.port_or_known_default()) else {
        return "invalid".into();
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{host}:{port}")
}

pub fn log_launch(
    origin: AgentLaunchOrigin,
    tab_id: &str,
    provider: &str,
    command: &str,
    prompt: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    prompt_delivery: &str,
) {
    log_launch_with_parameters(
        origin,
        tab_id,
        provider,
        command,
        prompt,
        role,
        role_profile,
        prompt_delivery,
        None,
        None,
    );
}

pub fn log_launch_with_parameters(
    origin: AgentLaunchOrigin,
    tab_id: &str,
    provider: &str,
    command: &str,
    prompt: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    prompt_delivery: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) {
    let prompt_bytes = prompt.map(str::len).unwrap_or(0);
    log::info!(
        "[agent-launch] origin={} tab={} provider={} role={} role_profile={} model={} effort={} prompt_delivery={} prompt_present={} prompt_bytes={} prompt_sha256={} command_bytes={} command_sha256={} child_exit=unavailable(pty) stderr=pty",
        origin.label(),
        tab_id,
        provider,
        role.unwrap_or("none"),
        role_profile.unwrap_or("none"),
        model.unwrap_or("none"),
        reasoning_effort.unwrap_or("none"),
        prompt_delivery,
        prompt.is_some(),
        prompt_bytes,
        digest(prompt.unwrap_or_default()),
        command.len(),
        digest(command),
    );
}

pub fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub fn log_shell_drop(
    tab_id: &str,
    pty_id: &str,
    output: &str,
    reason: &str,
    restarts: u32,
    max_restarts: u32,
) {
    let tail = tail_bytes(
        &codeterm_pty::pty::ansi::clean_terminal_output(output),
        TAIL_BYTES,
    );
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

pub fn tail_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut start = text.len() - max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

pub fn redact_secrets(input: &str) -> String {
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
mod tests;
