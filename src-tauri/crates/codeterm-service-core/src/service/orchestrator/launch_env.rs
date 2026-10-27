//! Shell prelude stamped onto an agent launch line.
//!
//! The PTY environment already puts the CodeTerm CLI directory first, but the
//! interactive shell loads its profile afterwards and can demote or drop it
//! (macOS `path_helper`, a PowerShell `$PROFILE` that rebuilds `$env:PATH`).
//! This line runs after that, so it is the last owner of the prepend and
//! applies the same rule as `path_with_cli_dir_first`: drop every existing
//! copy, then prepend once. A launched tab therefore carries the directory
//! exactly once end to end.

#[derive(Clone, Debug, Default)]
pub struct PreparedAgentCommand {
    pub shell_command: String,
    pub direct: Option<codeterm_types::agent::LaunchSpec>,
}

impl From<String> for PreparedAgentCommand {
    fn from(shell_command: String) -> Self {
        Self {
            shell_command,
            direct: None,
        }
    }
}

impl std::ops::Deref for PreparedAgentCommand {
    type Target = str;
    fn deref(&self) -> &str {
        &self.shell_command
    }
}

impl std::fmt::Display for PreparedAgentCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.shell_command)
    }
}

impl PreparedAgentCommand {
    pub(crate) fn with_execution_environment(
        mut self,
        policy: Option<&codeterm_types::plan::ExecutionSandboxPolicy>,
    ) -> Self {
        if self.shell_command.is_empty() {
            return self;
        }
        let Some(policy) = policy else {
            return self;
        };
        if let Some(spec) = &mut self.direct {
            spec.env_set.extend(policy.environment.clone());
            spec.env_unset
                .retain(|key| !policy.environment.contains_key(key));
        }
        let mut prefix = String::new();
        for (key, value) in &policy.environment {
            if cfg!(windows) {
                prefix.push_str(&format!("$env:{key}='{}'; ", value.replace('\'', "''")));
            } else {
                prefix.push_str(&format!(
                    "export {key}='{}'; ",
                    value.replace('\'', "'\"'\"'")
                ));
            }
        }
        self.shell_command = format!("{prefix}{}", self.shell_command);
        self
    }

    pub(crate) fn from_spec(spec: codeterm_types::agent::LaunchSpec) -> Self {
        let shell_command = std::iter::once(&spec.program)
            .chain(&spec.args)
            .map(|arg| codeterm_util::shell::ShellSyntax::quote_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            shell_command,
            direct: Some(spec),
        }
    }

    pub(crate) fn with_stub(mut self, provider: &str) -> Self {
        let command = crate::agent::stub_launch::apply(provider, self.shell_command.clone());
        if command != self.shell_command {
            self.direct = None;
            self.shell_command = command;
        }
        self
    }
}

pub(super) fn direct_environment(
    mut env: std::collections::HashMap<String, String>,
    spec: &codeterm_types::agent::LaunchSpec,
    nonce: &str,
    attempt_id: &str,
    cli_path: Option<&std::path::Path>,
    inherited_path: &std::ffi::OsStr,
) -> Result<std::collections::HashMap<String, String>, String> {
    let same_key = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let owned = [
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_STARTUP_ATTEMPT_ID",
        "PATH",
    ];
    if spec
        .env_unset
        .iter()
        .any(|key| owned.iter().any(|host| same_key(key, host)))
    {
        return Err("launch spec cannot remove host launch identity or PATH".into());
    }
    let credentials = ["CODETERM_API_TOKEN", "CODETERM_TAB_ID"];
    if spec.env_set.keys().chain(spec.env_unset.iter()).any(|key| credentials.iter().any(|host| same_key(key, host))) {
        return Err("launch spec cannot replace or remove tab credentials".into());
    }
    for (key, value) in &spec.env_set {
        env.retain(|existing, _| !same_key(existing, key));
        env.insert(key.clone(), value.clone());
    }
    let path = env
        .iter()
        .find(|(key, _)| same_key(key, "PATH"))
        .map(|(_, value)| std::ffi::OsString::from(value))
        .unwrap_or_else(|| inherited_path.to_owned());
    env.retain(|key, _| {
        !owned.iter().any(|host| same_key(key, host))
            && !spec.env_unset.iter().any(|unset| same_key(key, unset))
    });
    env.insert(owned[0].into(), nonce.into());
    env.insert(owned[1].into(), attempt_id.into());
    let path = if let Some(dir) =
        cli_path.and_then(|path| crate::pty::manager::cli_dir_of(path.as_os_str()))
    {
        crate::pty::manager::path_with_cli_dir_first(&path, &dir).ok_or("invalid launch PATH")?
    } else {
        path
    };
    env.insert(
        "PATH".into(),
        path.to_str().ok_or("launch PATH is not UTF-8")?.into(),
    );
    Ok(env)
}

fn powershell_cli_dir_prelude(dir: &str) -> String {
    let quoted = dir.replace('\'', "''");
    format!(
        "$env:PATH=(@('{quoted}') + (($env:PATH -split ';') | Where-Object {{ $_ -and $_ -ne '{quoted}' }})) -join ';'; "
    )
}

fn posix_cli_dir_prelude(dir: &str) -> String {
    let quoted = dir.replace('\'', "'\"'\"'");
    format!(
        "export PATH=\"$(IFS=:; printf %s '{quoted}'; for entry in $PATH; do [ \"$entry\" = '{quoted}' ] || printf ':%s' \"$entry\"; done)\"; "
    )
}

pub(super) fn command_with_binding_nonce_for_shell(
    command: &str,
    binding_nonce: &str,
    startup_attempt_id: &str,
    windows: bool,
    cli_path: Option<&std::ffi::OsStr>,
) -> String {
    let cli_dir = cli_path.and_then(crate::pty::manager::cli_dir_of);
    if windows {
        let binding = binding_nonce.replace('\'', "''");
        let startup_attempt = startup_attempt_id.replace('\'', "''");
        let path = cli_dir
            .map(|dir| powershell_cli_dir_prelude(&dir.to_string_lossy()))
            .unwrap_or_default();
        format!(
            "$env:CODETERM_SESSION_BINDING_NONCE='{binding}'; $env:CODETERM_STARTUP_ATTEMPT_ID='{startup_attempt}'; {path}{command}"
        )
    } else {
        let binding = binding_nonce.replace('\'', "'\"'\"'");
        let startup_attempt = startup_attempt_id.replace('\'', "'\"'\"'");
        let path = cli_dir
            .map(|dir| posix_cli_dir_prelude(&dir.to_string_lossy()))
            .unwrap_or_default();
        format!(
            "export CODETERM_SESSION_BINDING_NONCE='{binding}'; export CODETERM_STARTUP_ATTEMPT_ID='{startup_attempt}'; {path}{command}"
        )
    }
}

#[cfg(test)]
mod tests;
