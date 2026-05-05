//! `codeterm snip <cmd>...` — token-saving command wrapper.
//!
//! Spawn the user's command, capture stdout + stderr, route each stream
//! through a per-tool truncator that drops noise (passing tests, compile
//! spam, hunk middles) while preserving diagnostic signal. Inherit the
//! child's exit code so the agent still sees pass/fail.

pub mod cargo;
pub mod git;
pub mod go;
pub mod node;
pub mod parser;
pub mod python;

use std::io::Write;
use std::process::{Command, Stdio};

/// Entry point invoked from `main.rs` for `Commands::Snip`. Diverges via
/// `std::process::exit` after writing the truncated output.
pub fn handle_snip(cmd: Vec<String>) -> ! {
    let (env, head, args) = split_env_head_args(cmd);

    let head = match head {
        Some(h) => h,
        None => {
            eprintln!("codeterm snip: no command provided");
            std::process::exit(2);
        }
    };

    if parser::is_shell_builtin(&head) {
        eprintln!(
            "codeterm snip: refusing to wrap shell builtin '{}' (run it directly)",
            head
        );
        std::process::exit(2);
    }

    let mut command = Command::new(&head);
    command.args(&args);
    for (k, v) in &env {
        command.env(k, v);
    }
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let output = match command.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("codeterm snip: failed to spawn '{}': {}", head, e);
            std::process::exit(127);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let snipped_stdout = dispatch(&head, &args, &stdout);
    let snipped_stderr = dispatch(&head, &args, &stderr);

    let _ = std::io::stdout().write_all(snipped_stdout.as_bytes());
    let _ = std::io::stderr().write_all(snipped_stderr.as_bytes());

    std::process::exit(output.status.code().unwrap_or(1));
}

/// Pure-function dispatch — tested independently of process spawning.
pub fn dispatch(head: &str, args: &[String], input: &str) -> String {
    match head {
        "cargo" => cargo::snip(args, input),
        "go" => go::snip(args, input),
        "git" => git::snip(args, input),
        "npm" | "pnpm" | "yarn" | "jest" | "vitest" | "npx" => node::snip(head, args, input),
        "pytest" | "py.test" | "python" | "python3" => python::snip(head, args, input),
        _ => input.to_string(),
    }
}

/// Split `cmd` argv into (env_assignments, head, remaining_args). Leading
/// tokens of the form `IDENT=value` are pulled into the environment for
/// the spawned child, matching the way bash/zsh would have set them if
/// the user typed `KEY=val codeterm snip …` in the shell.
pub(crate) fn split_env_head_args(
    cmd: Vec<String>,
) -> (Vec<(String, String)>, Option<String>, Vec<String>) {
    let mut env: Vec<(String, String)> = Vec::new();
    let mut iter = cmd.into_iter();
    while let Some(tok) = iter.next() {
        if let Some((k, v)) = parse_env_token(&tok) {
            env.push((k, v));
            continue;
        }
        let args: Vec<String> = iter.collect();
        return (env, Some(tok), args);
    }
    (env, None, Vec::new())
}

fn parse_env_token(s: &str) -> Option<(String, String)> {
    let eq = s.find('=')?;
    let (k, rest) = s.split_at(eq);
    if !is_env_ident(k) {
        return None;
    }
    Some((k.to_string(), rest[1..].to_string()))
}

fn is_env_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return false;
    }
    bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn dispatch_unknown_head_is_passthrough() {
        let input = "any output";
        assert_eq!(dispatch("ruby", &[], input), input);
    }

    #[test]
    fn dispatch_cargo_test_truncates() {
        let input = "Compiling foo\nFinished `test` profile\nrunning 0 tests\ntest result: ok. 0 passed";
        let out = dispatch("cargo", &[s("test")], input);
        assert!(!out.contains("Compiling"));
        assert!(out.contains("test result"));
    }

    #[test]
    fn dispatch_go_test_truncates() {
        let input = "--- PASS: TestFoo\nok\tpkg/x\t0.1s";
        let out = dispatch("go", &[s("test")], input);
        assert_eq!(out, "");
    }

    #[test]
    fn dispatch_git_log_truncates_long() {
        let input = (0..200).map(|i| format!("c{}", i)).collect::<Vec<_>>().join("\n");
        let out = dispatch("git", &[s("log")], &input);
        assert_eq!(out.lines().count(), 35);
    }

    #[test]
    fn dispatch_jest_truncates() {
        let out = dispatch("jest", &[], "PASS src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn dispatch_vitest_truncates() {
        let out = dispatch("vitest", &[], "PASS src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn dispatch_pytest_truncates() {
        let out = dispatch("pytest", &[], "tests/x.py::t PASSED");
        assert_eq!(out, "");
    }

    #[test]
    fn dispatch_npm_test() {
        let out = dispatch("npm", &[s("test")], "PASS x");
        assert_eq!(out, "");
    }

    #[test]
    fn dispatch_pnpm_build() {
        let out = dispatch("pnpm", &[s("build")], "  50% [######    ] building");
        assert_eq!(out, "");
    }

    #[test]
    fn split_env_no_env() {
        let (env, head, args) = split_env_head_args(vec![s("cargo"), s("test")]);
        assert!(env.is_empty());
        assert_eq!(head.as_deref(), Some("cargo"));
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn split_env_one_env() {
        let (env, head, args) = split_env_head_args(vec![s("FOO=bar"), s("cargo"), s("test")]);
        assert_eq!(env, vec![(s("FOO"), s("bar"))]);
        assert_eq!(head.as_deref(), Some("cargo"));
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn split_env_multiple() {
        let (env, head, args) = split_env_head_args(vec![
            s("RUST_LOG=debug"),
            s("FOO=bar"),
            s("cargo"),
            s("test"),
        ]);
        assert_eq!(env.len(), 2);
        assert_eq!(env[0], (s("RUST_LOG"), s("debug")));
        assert_eq!(env[1], (s("FOO"), s("bar")));
        assert_eq!(head.as_deref(), Some("cargo"));
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn split_env_only_no_command() {
        let (env, head, args) = split_env_head_args(vec![s("FOO=bar")]);
        assert_eq!(env.len(), 1);
        assert_eq!(head, None);
        assert!(args.is_empty());
    }

    #[test]
    fn split_env_rejects_invalid_ident() {
        let (env, head, args) = split_env_head_args(vec![s("1FOO=bar"), s("cmd")]);
        assert!(env.is_empty());
        assert_eq!(head.as_deref(), Some("1FOO=bar"));
        assert_eq!(args, vec!["cmd"]);
    }

    #[test]
    fn split_env_empty_value() {
        let (env, head, args) = split_env_head_args(vec![s("FOO="), s("cmd")]);
        assert_eq!(env, vec![(s("FOO"), s(""))]);
        assert_eq!(head.as_deref(), Some("cmd"));
        assert!(args.is_empty());
    }

    #[test]
    fn split_env_path_value() {
        let (env, head, _) = split_env_head_args(vec![s("PATH=/a/b:/c/d"), s("cmd")]);
        assert_eq!(env, vec![(s("PATH"), s("/a/b:/c/d"))]);
        assert_eq!(head.as_deref(), Some("cmd"));
    }

    #[test]
    fn split_env_dash_in_arg_not_env() {
        let (env, head, args) = split_env_head_args(vec![s("--flag=value"), s("cmd")]);
        assert!(env.is_empty());
        assert_eq!(head.as_deref(), Some("--flag=value"));
        assert_eq!(args, vec!["cmd"]);
    }

    #[test]
    fn split_env_underscore_ident() {
        let (env, head, _) = split_env_head_args(vec![s("_PRIVATE=x"), s("cmd")]);
        assert_eq!(env, vec![(s("_PRIVATE"), s("x"))]);
        assert_eq!(head.as_deref(), Some("cmd"));
    }

    #[test]
    fn split_env_after_command_is_arg() {
        let (env, head, args) = split_env_head_args(vec![
            s("FOO=bar"),
            s("cargo"),
            s("test"),
            s("BAZ=qux"),
        ]);
        assert_eq!(env, vec![(s("FOO"), s("bar"))]);
        assert_eq!(head.as_deref(), Some("cargo"));
        assert_eq!(args, vec!["test", "BAZ=qux"]);
    }

    #[test]
    fn empty_argv_returns_no_head() {
        let (env, head, args) = split_env_head_args(vec![]);
        assert!(env.is_empty());
        assert_eq!(head, None);
        assert!(args.is_empty());
    }

    #[test]
    fn parse_env_token_basic() {
        assert_eq!(parse_env_token("FOO=bar"), Some(("FOO".into(), "bar".into())));
    }

    #[test]
    fn parse_env_token_no_equals() {
        assert_eq!(parse_env_token("FOOBAR"), None);
    }

    #[test]
    fn parse_env_token_empty() {
        assert_eq!(parse_env_token(""), None);
    }

    #[test]
    fn parse_env_token_only_equals() {
        assert_eq!(parse_env_token("="), None);
    }

    #[test]
    fn parse_env_token_value_with_equals() {
        assert_eq!(
            parse_env_token("FOO=bar=baz"),
            Some(("FOO".into(), "bar=baz".into()))
        );
    }

    #[test]
    fn is_env_ident_alpha() {
        assert!(is_env_ident("FOO"));
    }

    #[test]
    fn is_env_ident_alnum() {
        assert!(is_env_ident("FOO2"));
    }

    #[test]
    fn is_env_ident_underscore_start() {
        assert!(is_env_ident("_F"));
    }

    #[test]
    fn is_env_ident_rejects_digit_start() {
        assert!(!is_env_ident("1F"));
    }

    #[test]
    fn is_env_ident_rejects_dash() {
        assert!(!is_env_ident("F-O"));
    }

    #[test]
    fn is_env_ident_rejects_empty() {
        assert!(!is_env_ident(""));
    }

    #[test]
    fn is_env_ident_rejects_dot() {
        assert!(!is_env_ident("F.O"));
    }

    #[test]
    fn is_env_ident_lowercase_ok() {
        assert!(is_env_ident("foo"));
    }
}
