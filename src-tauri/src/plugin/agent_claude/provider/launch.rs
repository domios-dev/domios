//! Claude Code launch + resume command construction.

use crate::agent::provider::AgentLaunchParams;
use crate::shell::ShellSyntax;

pub(super) fn build_launch_command(params: &AgentLaunchParams) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(ShellSyntax::UNSET_CLAUDECODE.to_string());
    parts.push("claude".to_string());
    if params.skip_permissions {
        parts.push("--dangerously-skip-permissions".to_string());
    }

    if let Some(ref name) = params.agent_name {
        parts.push("--agent".to_string());
        parts.push(ShellSyntax::quote(name));
    }

    // Pre-generated session id (only honored for fresh sessions — `--session-id` is silently ignored by claude when also resuming).
    if let Some(ref sid) = params.session_id {
        parts.push("--session-id".to_string());
        parts.push(ShellSyntax::quote(sid));
    }

    if let Some(ref extra) = params.args {
        parts.extend(extra.iter().cloned());
    }

    // Prompt is a positional, last, after `--`: a spawn task can start with the `-=-codeterm:...-=-` origin marker, which commander would otherwise reject as an unknown option.
    if let Some(ref task) = params.task {
        parts.push("--".to_string());
        parts.push(ShellSyntax::task_file_expr(task));
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
            session_id: None,
            structured_prompt_server: None,
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
            session_id: None,
            structured_prompt_server: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains(".ct-task-"), "task via temp file");
        assert!(!cmd.contains(" -p"), "no headless -p mode anymore");
        assert!(
            cmd.contains(" -- "),
            "task positional must follow -- separator"
        );
        assert!(
            cmd.find(" -- ").unwrap() < cmd.find(".ct-task-").unwrap(),
            "-- must precede the task positional: {}",
            cmd
        );
    }

    #[test]
    fn test_build_launch_with_agent_name() {
        let params = AgentLaunchParams {
            agent_name: Some("architect".to_string()),
            task: None,
            args: None,
            skip_permissions: true,
            session_id: None,
            structured_prompt_server: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--agent"));
        assert!(cmd.contains("'architect'"));
    }

    #[test]
    fn test_build_launch_visible_task_positional_after_flags_and_separator() {
        let model_id = crate::agent::models::resolve_model_id("haiku");
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("hello".to_string()),
            args: Some(vec!["--model".into(), model_id.into()]),
            skip_permissions: true,
            session_id: None,
            structured_prompt_server: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains(".ct-task-"));
        assert!(cmd.contains(&format!("--model {}", model_id)));
        // Flags first, then `--`, then the task positional last.
        let model_pos = cmd.find("--model").unwrap();
        let sep_pos = cmd.find(" -- ").unwrap();
        let task_pos = cmd.find(".ct-task-").unwrap();
        assert!(
            model_pos < sep_pos && sep_pos < task_pos,
            "order must be flags -> -- -> task: {}",
            cmd
        );
    }

    #[test]
    fn test_build_launch_command_with_session_id_injection() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            session_id: Some("my-pre-generated-uuid".to_string()),
            structured_prompt_server: None,
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
    fn test_session_id_injected_even_with_task() {
        // Interactive launch always honors --session-id (the old -p path that ignored it is gone).
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("hello".to_string()),
            args: None,
            skip_permissions: true,
            session_id: Some("sid-xyz".to_string()),
            structured_prompt_server: None,
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--session-id"));
        assert!(cmd.contains("sid-xyz"));
        let sid_pos = cmd.find("--session-id").unwrap();
        let sep_pos = cmd.find(" -- ").unwrap();
        assert!(
            sid_pos < sep_pos,
            "session-id before task separator: {}",
            cmd
        );
    }

    #[test]
    fn test_build_resume_command() {
        let cmd = build_resume_command("abc-def-123-456", true).unwrap();
        assert!(cmd.contains("--resume"));
        assert!(cmd.contains("abc-def-123-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }
}
