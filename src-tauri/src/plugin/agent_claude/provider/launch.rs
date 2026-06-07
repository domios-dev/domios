//! Claude Code launch + resume command construction.

use crate::agent::provider::AgentLaunchParams;
use crate::shell::ShellSyntax;

pub(super) fn build_launch_command(params: &AgentLaunchParams) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(ShellSyntax::UNSET_CLAUDECODE.to_string());

    // Headless: stdin-pipe + -p, avoids PowerShell argv splitting on prompt --flags.
    let use_stdin_pipe = params.headless && params.task.is_some();

    if use_stdin_pipe {
        let task = params.task.as_ref().unwrap();
        let (pipe_prefix, _path) = ShellSyntax::task_file_stdin_pipe(task);
        parts.push(pipe_prefix);
        parts.push("claude".to_string());
        if params.skip_permissions {
            parts.push("--dangerously-skip-permissions".to_string());
        }
        // Keep Claude in streaming mode (-p) and allow Bash tools so extraction can call the CLI.
        // Without stream output + allowedTools, the extraction pipeline can stall.
        parts.push("-p".to_string());
        parts.push("--output-format".to_string());
        parts.push("stream-json".to_string());
        parts.push("--verbose".to_string());
        parts.push("--include-partial-messages".to_string());
        parts.push("--allowedTools".to_string());
        parts.push("Bash".to_string());
    } else {
        parts.push("claude".to_string());
        if params.skip_permissions {
            parts.push("--dangerously-skip-permissions".to_string());
        }
        if let Some(ref task) = params.task {
            parts.push(ShellSyntax::task_file_expr(task));
        }
    }

    if let Some(ref name) = params.agent_name {
        parts.push("--agent".to_string());
        parts.push(ShellSyntax::quote(name));
    }

    // Pre-generated session id (only honored for fresh sessions — `--session-id` is silently ignored by claude when also resuming).
    if !use_stdin_pipe {
        if let Some(ref sid) = params.session_id {
            parts.push("--session-id".to_string());
            parts.push(ShellSyntax::quote(sid));
        }
    }

    if let Some(ref extra) = params.args {
        parts.extend(extra.iter().cloned());
    }

    parts.join(" ")
}

pub(super) fn build_resume_command(session_id: &str, skip_permissions: bool) -> Option<String> {
    let mut parts = vec![
        ShellSyntax::UNSET_CLAUDECODE.to_string(),
        "claude".to_string(),
    ];
    if skip_permissions {
        parts.push("--dangerously-skip-permissions".to_string());
    }
    parts.push("--resume".to_string());
    parts.push(ShellSyntax::quote(session_id));
    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::AgentLaunchParams;

    #[test]
    fn test_build_launch_no_task() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        if cfg!(target_os = "windows") {
            assert!(cmd.starts_with("Remove-Item Env:CLAUDECODE"));
            assert!(cmd.contains("[System.Text.Encoding]::UTF8"));
            assert!(cmd.ends_with(" claude --dangerously-skip-permissions"));
        } else {
            assert_eq!(
                cmd,
                "unset CLAUDECODE && claude --dangerously-skip-permissions"
            );
        }
    }

    #[test]
    fn test_build_launch_with_task() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("count files".to_string()),
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains(".ct-task-"), "task via temp file");
        assert!(!cmd.contains(" -p "), "visible mode must not use -p");
    }

    #[test]
    fn test_build_launch_with_agent_name() {
        let params = AgentLaunchParams {
            agent_name: Some("architect".to_string()),
            task: None,
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--agent"));
        assert!(cmd.contains("'architect'"));
    }

    #[test]
    fn test_build_launch_headless_uses_stdin_pipe_and_print_mode() {
        let task = "codeterm mem save --kind fact --area codeterm \"body --flag value\"";
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some(task.to_string()),
            args: None,
            skip_permissions: true,
            headless: true,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        if cfg!(target_os = "windows") {
            assert!(
                cmd.contains("Get-Content "),
                "headless must use Get-Content pipe: {}",
                cmd
            );
            assert!(
                cmd.contains("-Raw -Encoding UTF8 |"),
                "missing pipe: {}",
                cmd
            );
        } else {
            assert!(cmd.contains("cat "), "headless must use cat pipe: {}", cmd);
            assert!(cmd.contains(" |"), "missing pipe: {}", cmd);
        }
        assert!(
            cmd.contains(".ct-task-"),
            "task file path expected: {}",
            cmd
        );
        assert!(cmd.contains("claude"), "claude bin: {}", cmd);
        assert!(cmd.contains(" -p"), "headless must add -p: {}", cmd);
        assert!(
            cmd.contains("--output-format stream-json"),
            "headless must request stream-json: {}",
            cmd
        );
        assert!(
            cmd.contains("--verbose"),
            "stream-json with -p requires --verbose: {}",
            cmd
        );
        assert!(
            cmd.contains("--allowedTools Bash"),
            "headless must allow Bash tool for multi-turn: {}",
            cmd
        );
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(
            !cmd.contains("(Get-Content"),
            "headless must NOT use inline subexpression: {}",
            cmd
        );
        let after_p = cmd.split(" -p").nth(1).unwrap_or("");
        assert!(
            !after_p.contains(" --area "),
            "task body --area must not leak into argv: {}",
            cmd
        );
    }

    #[test]
    fn test_build_launch_headless_no_task_falls_back_to_legacy() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            headless: true,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        assert!(!cmd.contains(" -p"), "no -p without task: {}", cmd);
        assert!(
            !cmd.contains("--output-format"),
            "no stream-json without task: {}",
            cmd
        );
        assert!(!cmd.contains("Get-Content"));
        assert!(!cmd.contains("cat "));
        assert!(cmd.contains("claude"));
    }

    #[test]
    fn test_build_launch_visible_with_args_after_task() {
        let model_id = crate::agent::models::resolve_model_id("haiku");
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("hello".to_string()),
            args: Some(vec!["--model".into(), model_id.into()]),
            skip_permissions: true,
            headless: false,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains(".ct-task-"));
        assert!(cmd.contains(&format!("--model {}", model_id)));
    }

    #[test]
    fn test_build_launch_headless_with_extra_args() {
        let model_id = crate::agent::models::resolve_model_id("haiku");
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("do thing --kind fact".to_string()),
            args: Some(vec!["--model".into(), model_id.into()]),
            skip_permissions: true,
            headless: true,
            session_id: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains(" -p"));
        assert!(cmd.contains("--output-format stream-json"));
        assert!(cmd.contains("--verbose"));
        assert!(cmd.contains(&format!("--model {}", model_id)));
        let claude_pos = cmd.find(" claude").expect("claude in cmd");
        if cfg!(target_os = "windows") {
            let pipe_pos = cmd.find("Get-Content").expect("pipe in cmd");
            assert!(pipe_pos < claude_pos);
        } else {
            let pipe_pos = cmd.find("cat ").expect("pipe in cmd");
            assert!(pipe_pos < claude_pos);
        }
    }

    #[test]
    fn test_build_launch_command_with_session_id_injection() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: Some("my-pre-generated-uuid".to_string()),
        };
        let cmd = build_launch_command(&params);
        assert!(
            cmd.contains("--session-id"),
            "must pass --session-id flag, got: {}",
            cmd
        );
        assert!(
            cmd.contains("my-pre-generated-uuid"),
            "must include the chosen uuid, got: {}",
            cmd
        );
    }

    #[test]
    fn test_headless_does_not_inject_session_id() {
        // Headless (-p) Claude launches don't take --session-id; we must skip injection rather than emit a flag claude will reject.
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("hello".to_string()),
            args: None,
            skip_permissions: true,
            headless: true,
            session_id: Some("should-not-appear".to_string()),
        };
        let cmd = build_launch_command(&params);
        assert!(
            !cmd.contains("--session-id"),
            "headless launch must not pass --session-id, got: {}",
            cmd
        );
        assert!(!cmd.contains("should-not-appear"));
    }

    #[test]
    fn test_build_resume_command() {
        let cmd = build_resume_command("abc-def-123-456", true).unwrap();
        assert!(cmd.contains("--resume"));
        assert!(cmd.contains("abc-def-123-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }
}
