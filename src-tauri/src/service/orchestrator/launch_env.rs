//! Shell prelude stamped onto an agent launch line.
//!
//! The PTY environment already puts the CodeTerm CLI directory first, but the
//! interactive shell loads its profile afterwards and can demote or drop it
//! (macOS `path_helper`, a PowerShell `$PROFILE` that rebuilds `$env:PATH`).
//! This line runs after that, so it is the last owner of the prepend and
//! applies the same rule as `path_with_cli_dir_first`: drop every existing
//! copy, then prepend once. A launched tab therefore carries the directory
//! exactly once end to end.

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
