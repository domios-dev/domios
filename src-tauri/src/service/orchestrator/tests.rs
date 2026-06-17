use super::*;
use crate::shell::ShellSyntax;

fn make_params(agent: &str, task: Option<&str>) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(agent.to_string()),
        task: task.map(|t| t.to_string()),
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        resume_session_id: None,
        activate: None,
    }
}

fn test_registry() -> AgentRegistry {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    reg
}

#[test]
fn test_build_claude_with_task() {
    let reg = test_registry();
    let params = make_params("claude", Some("count files"));
    let cmd = build_agent_command("claude", &params, Some("count files"), None, &reg);
    assert!(cmd.contains("--dangerously-skip-permissions"));
    assert!(cmd.contains(".ct-task-"), "task via temp file");
    if cfg!(target_os = "windows") {
        assert!(cmd.contains("Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue;"));
        assert!(cmd.contains("(Get-Content"));
    } else {
        assert!(cmd.contains("unset CLAUDECODE &&"));
        assert!(cmd.contains("$(cat /tmp/"));
    }
}

#[test]
fn test_build_claude_no_task() {
    let reg = test_registry();
    let params = make_params("claude", None);
    let cmd = build_agent_command("claude", &params, None, None, &reg);
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
fn test_build_claude_with_agent_name() {
    let reg = test_registry();
    let mut params = make_params("claude", Some("review code"));
    params.agent_name = Some("architect".to_string());
    let cmd = build_agent_command("claude", &params, None, None, &reg);
    assert!(cmd.contains("--agent 'architect'"));
}

#[test]
fn test_build_other_agent() {
    let reg = test_registry();
    let mut params = make_params("aider", None);
    params.args = Some(vec!["--model".to_string(), "gpt-4".to_string()]);
    let cmd = build_agent_command("aider", &params, None, None, &reg);
    assert_eq!(cmd, "aider --model gpt-4");
    assert!(!cmd.contains("CLAUDECODE"));
}

#[test]
fn test_shell_quote() {
    assert_eq!(ShellSyntax::quote("hello"), "'hello'");
    assert_eq!(
        ShellSyntax::quote("count *.tsx files"),
        "'count *.tsx files'"
    );
    if cfg!(target_os = "windows") {
        assert_eq!(ShellSyntax::quote("it's"), "'it''s'");
    } else {
        assert_eq!(ShellSyntax::quote("it's"), "'it'\\''s'");
    }
}

// ── 4. build_agent_command with special chars ──

#[test]
fn test_build_claude_task_with_newlines() {
    let reg = test_registry();
    let params = make_params("claude", Some("line1\nline2\nline3"));
    let cmd = build_agent_command("claude", &params, Some("line1\nline2\nline3"), None, &reg);
    // Task goes through temp file, so newlines are preserved in the file
    assert!(
        cmd.contains(".ct-task-"),
        "task with newlines still uses temp file"
    );
    assert!(cmd.contains("--dangerously-skip-permissions"));
}

#[test]
fn test_build_claude_task_with_single_quotes() {
    let reg = test_registry();
    let task = "count files that don't match '*.rs'";
    let params = make_params("claude", Some(task));
    let cmd = build_agent_command("claude", &params, Some(task), None, &reg);
    // Task is written to a temp file, so quotes don't break the command
    assert!(cmd.contains(".ct-task-"));
}

#[test]
fn test_build_claude_task_with_double_quotes() {
    let reg = test_registry();
    let task = r#"find files matching "*.rs""#;
    let params = make_params("claude", Some(task));
    let cmd = build_agent_command("claude", &params, Some(task), None, &reg);
    assert!(cmd.contains(".ct-task-"));
}

#[test]
fn test_build_claude_task_with_shell_special_chars() {
    let reg = test_registry();
    let task = "echo $HOME && ls | grep foo; rm -rf /tmp/test";
    let params = make_params("claude", Some(task));
    let cmd = build_agent_command("claude", &params, Some(task), None, &reg);
    // Should not contain raw special chars in the command itself — they're in the temp file
    assert!(cmd.contains(".ct-task-"));
    assert!(
        !cmd.contains("$HOME"),
        "shell vars should be in temp file, not in command"
    );
}

#[test]
fn test_build_claude_task_temp_file_created() {
    let reg = test_registry();
    let task = "test task content";
    let params = make_params("claude", Some(task));
    let cmd = build_agent_command("claude", &params, Some(task), None, &reg);
    // Extract the temp file path from the command and verify it was written
    if cfg!(not(target_os = "windows")) {
        // Command looks like: ... "$(cat /tmp/.ct-task-XXXXXXXX)"
        let start = cmd.find("/tmp/.ct-task-").unwrap();
        let end = cmd[start..].find(')').unwrap() + start;
        let path = &cmd[start..end];
        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content, task, "temp file should contain the task verbatim");
        // Cleanup
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn test_build_claude_with_extra_args() {
    let reg = test_registry();
    let mut params = make_params("claude", None);
    params.args = Some(vec!["--verbose".to_string(), "--no-cache".to_string()]);
    let cmd = build_agent_command("claude", &params, None, None, &reg);
    assert!(cmd.contains("--verbose"));
    assert!(cmd.contains("--no-cache"));
    assert!(cmd.ends_with("--verbose --no-cache"));
}

#[test]
fn test_build_with_model_uses_provider_specific_flag() {
    // claude → `--model X`. codex/opencode model-flag syntax is JS-plugin logic now
    // (covered in plugins/provider-*/tests); here we verify the orchestrator
    // delegates model selection to the native claude provider.
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    let mut p = make_params("claude", None);
    p.model = Some("opus".into());
    let cmd = build_agent_command("claude", &p, None, None, &reg);
    assert!(
        cmd.contains("--model opus"),
        "claude expected --model X: {}",
        cmd
    );
}

#[test]
fn test_build_with_resume_session_id_uses_provider_resume_form() {
    // codex/opencode resume forms are JS-plugin logic now (plugins/provider-*/tests);
    // here we verify the orchestrator delegates resume to the native claude provider.
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    let mut p = make_params("claude", None);
    p.resume_session_id = Some("abc-123".into());
    let cmd = build_agent_command("claude", &p, None, None, &reg);
    assert!(
        cmd.contains("--resume") && cmd.contains("abc-123"),
        "claude resume: {}",
        cmd
    );
}

#[test]
fn test_build_non_claude_ignores_task_in_command() {
    let reg = test_registry();
    // For non-claude agents, build_agent_command does NOT embed the task
    let params = make_params("aider", Some("review this code"));
    let cmd = build_agent_command("aider", &params, Some("review this code"), None, &reg);
    // Non-claude: task is NOT added to the command (it's sent separately via PTY) The function just puts agent_cmd + args
    assert_eq!(cmd, "aider");
}
