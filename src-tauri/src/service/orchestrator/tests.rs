use super::*;
use crate::shell::ShellSyntax;

#[test]
fn cross_mesh_worker_skips_local_role_assignment() {
    use crate::multiplexer::types::AgentRole;
    // Regression: mesh `--role Worker` 500'd via set_pane_role(Worker, None).
    // A mesh report target + no local parent must skip local role assignment.
    assert!(skip_local_worker_role(
        &AgentRole::Worker,
        None,
        Some("Mac-Air")
    ));
}

#[test]
fn local_and_orchestrator_roles_keep_local_assignment() {
    use crate::multiplexer::types::AgentRole;
    // Locally-parented worker → keep hierarchy.
    assert!(!skip_local_worker_role(
        &AgentRole::Worker,
        Some("parent-pane"),
        Some("Mac-Air")
    ));
    // Worker with no mesh peer (purely local) → keep (and let it error as before).
    assert!(!skip_local_worker_role(&AgentRole::Worker, None, None));
    assert!(!skip_local_worker_role(&AgentRole::Worker, None, Some("")));
    // Orchestrator never needs a parent → never skipped here.
    assert!(!skip_local_worker_role(
        &AgentRole::Orchestrator,
        None,
        Some("Mac-Air")
    ));
    assert!(!skip_local_worker_role(
        &AgentRole::Manager,
        Some("parent-pane"),
        Some("Mac-Air")
    ));
}

#[test]
fn roleless_spawn_task_stays_plain() {
    assert_eq!(
        role_onboarding_task(Some("inspect"), None, None).as_deref(),
        Some("inspect")
    );
    assert_eq!(role_onboarding_task(None, None, None), None);
}

#[test]
fn worker_onboarding_requires_a_task_body() {
    assert!(role_onboarding_task(Some("fix it"), Some("Worker"), None)
        .unwrap()
        .contains(crate::prompts::WORKER_SUFFIX));
    assert_eq!(role_onboarding_task(None, Some("Worker"), None), None);
}

#[test]
fn planner_profile_uses_planner_suffix() {
    let task = role_onboarding_task(Some("plan it"), Some("Manager"), Some("planner")).unwrap();
    assert!(task.contains(&crate::prompts::planner_suffix()));
    assert!(task.contains(crate::prompts::PLANNER_TESTING_ORACLE_FIRST));
    assert!(task.contains(crate::prompts::PLAN_SKILLS_PRINCIPLES));
    assert!(!task.contains("{planner_testing_methodology}"));
    assert!(!task.contains(crate::prompts::WORKER_SUFFIX));
}

#[test]
fn manager_watcher_does_not_get_worker_suffix() {
    let task = role_onboarding_task(Some("watch it"), Some("Manager"), Some("watcher")).unwrap();
    assert_eq!(task, "watch it");
}

#[test]
fn orchestrator_onboarding_is_sent_even_without_task_body() {
    let task = role_onboarding_task(None, Some("Orchestrator"), None).unwrap();
    assert!(task.contains("CodeTerm planner is the source of truth"));
    assert!(task.contains(&crate::prompts::orchestrator_suffix()));
    assert!(task.contains(crate::prompts::PLAN_SKILLS_PRINCIPLES));
    assert!(!task.contains("{plan_skills_principles}"));
}

fn make_params(agent: &str, task: Option<&str>) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(agent.to_string()),
        task: task.map(|t| t.to_string()),
        task_id: None,
        plan_task: None,
        allow_shared_checkout: None,
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        title_provenance: None,
        description: None,
        role: None,
        role_profile: None,
        parent_pane_id: None,
        plan_id: None,
        can_spawn: None,
        workspace_id: None,
        force_tab: None,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        planned_model: None,
        reasoning_effort: None,
        system_prompt: None,
        preset: None,
        resume_session_id: None,
        charter: None,
        tick_interval_secs: None,
        nudge_cooldown_secs: None,
        max_actions_per_tick: None,
        activate: None,
        structured_prompt_server: None,
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
    assert!(cmd.contains("--append-system-prompt-file"));
    assert!(cmd.contains("Execute the CodeTerm task provided in the appended system prompt"));
    if cfg!(target_os = "windows") {
        assert!(cmd.contains("Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue;"));
    } else {
        assert!(cmd.contains("unset CLAUDECODE &&"));
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

fn effort_registry() -> AgentRegistry {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "flat",
    )));
    reg.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_reasoning_efforts(
            "tiered",
            &["low", "medium", "high"],
        ),
    ));
    reg
}

#[test]
fn build_agent_command_injects_supported_reasoning_effort() {
    let reg = effort_registry();
    let mut params = make_params("tiered", None);
    params.reasoning_effort = Some("high".into());
    let cmd = build_agent_command("tiered", &params, None, None, &reg);
    assert!(
        cmd.contains("--effort high"),
        "supported effort must reach the launch command: {cmd}"
    );
}

#[test]
fn build_agent_command_omits_effort_for_flat_provider() {
    // A provider with no effort capability adds no flag even if one is passed —
    // the launch builder never fabricates a flag the CLI would reject. (The
    // dispatch path rejects this upstream via validate_agent_reasoning_effort.)
    let reg = effort_registry();
    let mut params = make_params("flat", None);
    params.reasoning_effort = Some("high".into());
    let cmd = build_agent_command("flat", &params, None, None, &reg);
    assert!(
        !cmd.contains("--effort"),
        "flat provider adds no effort flag"
    );
}

#[test]
fn validate_reasoning_effort_gates_dispatch() {
    use crate::service::spawn::validate_agent_reasoning_effort;
    let reg = effort_registry();
    // No selection is always valid.
    assert!(validate_agent_reasoning_effort(&reg, "tiered", None).is_ok());
    assert!(validate_agent_reasoning_effort(&reg, "tiered", Some("")).is_ok());
    // Supported level passes.
    assert!(validate_agent_reasoning_effort(&reg, "tiered", Some("medium")).is_ok());
    // Unsupported level is rejected structurally — no silent downgrade.
    let err = validate_agent_reasoning_effort(&reg, "tiered", Some("ultra"))
        .expect_err("unsupported effort must reject");
    assert!(
        err.contains("low, medium, high"),
        "error lists support: {err}"
    );
    // A provider with no capability rejects any explicit effort.
    assert!(validate_agent_reasoning_effort(&reg, "flat", Some("high")).is_err());
    // Unknown provider cannot be validated → error, never a silent pass.
    assert!(validate_agent_reasoning_effort(&reg, "ghost", Some("high")).is_err());
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
    if cfg!(not(target_os = "windows")) {
        // Command looks like: ... "$(cat /tmp/.ct-task-XXXXXXXX)"
        let start = cmd.find("/tmp/.ct-task-").unwrap();
        let end = cmd[start..].find(')').unwrap() + start;
        let path = &cmd[start..end];
        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content, task, "temp file should contain the task verbatim");
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
fn build_agent_command_canonicalizes_display_model_before_launch_args() {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    crate::agent::model_catalog::register(
        "display-canon",
        crate::agent::model_catalog::RegisteredDefaults {
            models: vec![crate::agent::provider::ModelInfo {
                id: "gpt-5.5".into(),
                display_name: "GPT-5.5".into(),
                description: None,
                available: None,
            }],
            default: Some("gpt-5.5".into()),
        },
    );

    let mut p = make_params("display-canon", None);
    p.model = Some("GPT-5.5".into());
    let cmd = build_agent_command("display-canon", &p, None, None, &reg);

    assert!(
        cmd.contains("--model gpt-5.5"),
        "launch command must use canonical id: {}",
        cmd
    );
    assert!(
        !cmd.contains("--model GPT-5.5"),
        "launch command leaked display name: {}",
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
    let params = make_params("aider", Some("review this code"));
    let cmd = build_agent_command("aider", &params, Some("review this code"), None, &reg);
    assert_eq!(cmd, "aider");
}
