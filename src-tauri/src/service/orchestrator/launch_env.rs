pub(super) fn command_with_binding_nonce_for_shell(
    command: &str,
    binding_nonce: &str,
    startup_attempt_id: &str,
    windows: bool,
    cli_path: Option<&std::ffi::OsStr>,
) -> String {
    let cli_dir = cli_path
        .map(std::path::Path::new)
        .and_then(std::path::Path::parent)
        .filter(|path| !path.as_os_str().is_empty());
    if windows {
        let binding = binding_nonce.replace('\'', "''");
        let startup_attempt = startup_attempt_id.replace('\'', "''");
        let path = cli_dir.map(|dir| {
            format!(
                "$env:PATH='{};' + $env:PATH; ",
                dir.to_string_lossy().replace('\'', "''")
            )
        });
        format!(
            "$env:CODETERM_SESSION_BINDING_NONCE='{binding}'; $env:CODETERM_STARTUP_ATTEMPT_ID='{startup_attempt}'; {}{command}",
            path.unwrap_or_default()
        )
    } else {
        let binding = binding_nonce.replace('\'', "'\"'\"'");
        let startup_attempt = startup_attempt_id.replace('\'', "'\"'\"'");
        let path = cli_dir.map(|dir| {
            format!(
                "export PATH='{}':\"$PATH\"; ",
                dir.to_string_lossy().replace('\'', "'\"'\"'")
            )
        });
        format!(
            "export CODETERM_SESSION_BINDING_NONCE='{binding}'; export CODETERM_STARTUP_ATTEMPT_ID='{startup_attempt}'; {}{command}",
            path.unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_cli_directory_is_restored_after_shell_profile_loading() {
        let command = command_with_binding_nonce_for_shell(
            "claude",
            "nonce",
            "pty-a",
            false,
            Some(std::ffi::OsStr::new("/dev/bin/codeterm")),
        );
        assert_eq!(
            command,
            "export CODETERM_SESSION_BINDING_NONCE='nonce'; export CODETERM_STARTUP_ATTEMPT_ID='pty-a'; export PATH='/dev/bin':\"$PATH\"; claude"
        );
    }
}
