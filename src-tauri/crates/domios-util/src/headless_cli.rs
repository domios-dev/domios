use std::io::Write;
use std::process::{Command, Output, Stdio};

const CLAUDE_HEADLESS_SETTINGS: &str = r#"{"disableAllHooks":true}"#;
const EMPTY_MCP_CONFIG: &str = r#"{"mcpServers":{}}"#;

/// Flags for a one-shot `claude --print` call that must not touch the user's sessions,
/// hooks, MCP servers, skills or tools. `--bare` is deliberately absent: it never reads OAuth.
pub fn claude_headless_args() -> Vec<&'static str> {
    vec![
        "--print",
        "--settings",
        CLAUDE_HEADLESS_SETTINGS,
        "--no-session-persistence",
        "--strict-mcp-config",
        "--mcp-config",
        EMPTY_MCP_CONFIG,
        "--disable-slash-commands",
        "--tools",
        "",
    ]
}

/// Flags for a one-shot `codex exec` call that writes no session file, runs no hooks and
/// cannot modify files.
pub fn codex_exec_headless_args() -> Vec<&'static str> {
    vec![
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--disable",
        "hooks",
    ]
}

/// Environment for a headless provider call: it never inherits a tab identity and never
/// reaches out to claude.ai MCP servers or an IDE.
pub fn apply_headless_env(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CODETERM_") {
            cmd.env_remove(key);
        }
    }
    cmd.env_remove("FORCE_CODE_TERMINAL")
        .env("ENABLE_CLAUDEAI_MCP_SERVERS", "false")
        .env("CLAUDE_CODE_AUTO_CONNECT_IDE", "0")
        .env("CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL", "1")
        .current_dir(std::env::temp_dir());
}

/// Run `cmd` with `input` on stdin; the prompt travels by pipe because Windows `.cmd`
/// shims cannot carry a long multi-line argument.
pub fn output_with_stdin(cmd: &mut Command, input: &str) -> std::io::Result<Output> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("child stdin unavailable"))?;
    let payload = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&payload));
    let output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| std::io::Error::other("stdin writer panicked"))??;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(cmd: &Command, key: &str) -> Option<Option<String>> {
        cmd.get_envs()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn claude_args_never_use_bare_and_disable_hooks_and_persistence() {
        let args = claude_headless_args();
        assert!(!args.contains(&"--bare"));
        assert!(args.contains(&"--no-session-persistence"));
        let settings = args
            .iter()
            .position(|a| *a == "--settings")
            .map(|i| args[i + 1])
            .unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(settings).unwrap_or_default();
        assert_eq!(parsed["disableAllHooks"], serde_json::Value::Bool(true));
    }

    #[test]
    fn codex_args_are_ephemeral_and_read_only() {
        let args = codex_exec_headless_args();
        assert_eq!(args.first(), Some(&"exec"));
        assert!(args.contains(&"--ephemeral"));
        assert!(args.contains(&"--skip-git-repo-check"));
        let disabled = args
            .iter()
            .position(|a| *a == "--disable")
            .map(|i| args[i + 1]);
        assert_eq!(disabled, Some("hooks"));
        let sandbox = args
            .iter()
            .position(|a| *a == "--sandbox")
            .map(|i| args[i + 1]);
        assert_eq!(sandbox, Some("read-only"));
    }

    #[test]
    fn headless_env_strips_tab_identity_and_isolates_cwd() {
        let mut cmd = Command::new("probe");
        std::env::set_var("CODETERM_HEADLESS_CLI_TEST", "1");
        apply_headless_env(&mut cmd);
        std::env::remove_var("CODETERM_HEADLESS_CLI_TEST");
        assert_eq!(env_of(&cmd, "CODETERM_HEADLESS_CLI_TEST"), Some(None));
        assert_eq!(env_of(&cmd, "FORCE_CODE_TERMINAL"), Some(None));
        assert_eq!(
            env_of(&cmd, "ENABLE_CLAUDEAI_MCP_SERVERS"),
            Some(Some("false".into()))
        );
        assert_eq!(cmd.get_current_dir(), Some(std::env::temp_dir().as_path()));
    }
}
