use super::*;
use crate::shell::ShellSyntax;

#[test]
fn cross_mesh_worker_skips_local_role_assignment() {
    use crate::multiplexer::types::AgentRole;
    // Regression: mesh `--role Worker` 500'd via set_tab_role(Worker, None).
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
        Some("parent-tab"),
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
        Some("parent-tab"),
        Some("Mac-Air")
    ));
}

#[test]
fn roleless_spawn_task_stays_plain() {
    let task = role_onboarding_task(Some("inspect"), None, None).unwrap();
    assert!(task.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(task.ends_with("inspect"));
    let primer = role_onboarding_task(None, None, None).unwrap();
    assert!(primer.starts_with(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(primer.contains(crate::prompts::RICH_RENDERING_GUIDANCE.trim()));
}

#[test]
fn ordinary_tab_receives_primer_only_through_a_system_prompt_channel() {
    let capable = role_launch_task(None, None, None, true).expect("system primer");
    assert!(capable.starts_with(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(capable.contains(crate::prompts::RICH_RENDERING_GUIDANCE.trim()));
    assert_eq!(role_launch_task(None, None, None, false), None);

    let fallback = role_launch_task(Some("inspect"), None, None, false).expect("task");
    assert_eq!(fallback, "inspect");
    assert!(!fallback.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
}

#[test]
fn team_assignment_keeps_codeterm_context_on_fallback_providers() {
    let worker = role_launch_task(Some("repair delivery"), Some("Worker"), None, false).unwrap();
    assert!(worker.starts_with("You are a CodeTerm Worker."));
    assert!(worker.contains(crate::prompts::WORKER_CODETERM_CONTEXT.trim()));
    assert!(worker.contains(crate::prompts::WORKER_SUFFIX.trim()));
    assert_eq!(
        worker
            .matches(crate::prompts::WORKER_CODETERM_CONTEXT.trim())
            .count(),
        1
    );

    let capable = role_launch_task(Some("repair delivery"), Some("Worker"), None, true).unwrap();
    assert_eq!(
        capable
            .matches(crate::prompts::WORKER_CODETERM_CONTEXT.trim())
            .count(),
        1
    );
    assert_eq!(
        capable
            .matches(crate::prompts::WORKER_SUFFIX.trim())
            .count(),
        1
    );
}

#[test]
fn every_team_role_receives_the_shared_primer_exactly_once() {
    let cases = [
        (Some("Orchestrator"), None),
        (Some("Worker"), None),
        (Some("Manager"), None),
        (Some("Manager"), Some("planner")),
    ];
    for (role, profile) in cases {
        for system_prompt_capable in [false, true] {
            let prompt = role_launch_task(
                Some("team assignment"),
                role,
                profile,
                system_prompt_capable,
            )
            .unwrap();
            let context = if role == Some("Worker") {
                crate::prompts::WORKER_CODETERM_CONTEXT.trim()
            } else {
                crate::prompts::AGENT_CODETERM_PRIMER.trim()
            };
            assert_eq!(prompt.matches(context).count(), 1);
            assert_eq!(
                prompt
                    .matches(crate::prompts::RICH_RENDERING_GUIDANCE.trim())
                    .count(),
                1
            );
            assert_eq!(prompt.matches("team assignment").count(), 1);
        }
    }
}

#[test]
fn worker_without_task_identity_still_leads_with_a_generic_identity() {
    assert!(role_onboarding_task(Some("fix it"), Some("Worker"), None)
        .unwrap()
        .contains(crate::prompts::WORKER_SUFFIX.trim()));
    let prompt = role_onboarding_task(None, Some("Worker"), None).unwrap();
    assert!(prompt.starts_with("You are a CodeTerm Worker."));
    assert!(prompt.contains(crate::prompts::WORKER_CODETERM_CONTEXT.trim()));
}

#[test]
fn planner_profile_uses_planner_suffix() {
    let task = role_onboarding_task(Some("plan it"), Some("Manager"), Some("planner")).unwrap();
    assert!(task.contains(&crate::prompts::planner_suffix()));
    assert!(!task.contains(crate::prompts::WORKER_SUFFIX));
    assert_eq!(
        task.matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .count(),
        1
    );
    assert!(
        task.find(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .unwrap()
            < task.find("plan it").unwrap()
    );
}

#[test]
fn worker_prompt_orders_identity_context_task_commands_and_skill_fallback() {
    let brief = crate::service::plan::worker_identity::attach_worker_identity(
        "You are Hopper — executor for: repair delivery",
        &format!(
            "# Repair delivery\n\nFix the ledger.\n\n{}",
            crate::service::plan::worker_identity::WORKER_ACK_INSTRUCTION
        ),
    );
    let task = role_onboarding_task(Some(&brief), Some("Worker"), None).unwrap();
    let identity = task.find("You are Hopper").unwrap();
    let context = task
        .find(crate::prompts::WORKER_CODETERM_CONTEXT.trim())
        .unwrap();
    let assignment = task.find("## Assignment").unwrap();
    let brief = task.find("# Repair delivery").unwrap();
    let ack = task.find("codeterm agent ack").unwrap();
    let report = task
        .find("codeterm agent report --status in_review")
        .unwrap();
    let fallback = task
        .find("load the provider skill named `codeterm`")
        .unwrap();

    assert!(identity < context);
    assert!(context < assignment && assignment < brief);
    assert!(brief < ack && ack < report && report < fallback);
    assert!(!task.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(!task.contains(crate::service::plan::worker_identity::WORKER_IDENTITY_END));
    assert!(task.ends_with(
        "If a required CodeTerm command is not covered above, load the provider skill named `codeterm` as the fallback reference."
    ));
}

#[test]
fn manager_watcher_does_not_get_worker_suffix() {
    let task = role_onboarding_task(Some("watch it"), Some("Manager"), Some("watcher")).unwrap();
    assert!(task.ends_with("watch it"));
    assert!(!task.contains(crate::prompts::WORKER_SUFFIX));
}

#[test]
fn plain_orchestrator_no_task_spawn_receives_primer_and_role_contract() {
    let prompt =
        role_onboarding_task(None, Some("Orchestrator"), None).expect("orchestrator prompt");
    assert!(prompt.starts_with(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(prompt.contains(crate::prompts::ORCHESTRATOR_SUFFIX.trim()));
    assert_eq!(
        prompt
            .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .count(),
        1
    );

    let mut params = make_params("codex", None);
    params.role = Some("Orchestrator".into());
    let spawned = spawn_onboarding_task(&params).expect("orchestrator system prompt");
    assert_eq!(
        spawned
            .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .count(),
        1
    );
    assert!(spawned.contains(crate::prompts::ORCHESTRATOR_SUFFIX.trim()));
}

#[test]
fn paired_orchestrator_starts_with_system_contract_without_a_synthetic_turn() {
    let mut params = make_params("codex", None);
    params.role = Some("Orchestrator".into());
    params.skip_orchestrator_onboarding = Some(true);
    let prompt = spawn_onboarding_task(&params).expect("orchestrator system prompt");
    assert!(prompt.contains(crate::prompts::ORCHESTRATOR_SUFFIX.trim()));
}

#[test]
fn explicit_orchestrator_task_still_starts_an_execution_turn() {
    let mut params = make_params("codex", Some("inspect the launch path"));
    params.role = Some("Orchestrator".into());
    params.skip_orchestrator_onboarding = Some(true);
    let task = spawn_onboarding_task(&params).expect("explicit task");
    assert_eq!(task.matches("inspect the launch path").count(), 1);
    assert!(
        task.find("inspect the launch path").unwrap()
            < task
                .find(crate::prompts::ORCHESTRATOR_SUFFIX.trim())
                .unwrap()
    );
}

#[test]
fn fresh_spawn_paths_keep_shared_invariants_and_orchestrator_onboarding() {
    let orchestrator_spawn = include_str!("spawn.rs");
    let service_spawn = include_str!("../spawn.rs");

    assert!(!orchestrator_spawn.contains("maybe_deliver_orchestrator_promotion_onboarding"));
    assert!(!service_spawn.contains("maybe_deliver_orchestrator_promotion_onboarding"));
    assert!(orchestrator_spawn.contains("launch_agent_in_background("));
    assert!(service_spawn.contains("launch_agent_in_background_with_env("));
    assert!(
        orchestrator_spawn.contains("spawn_onboarding_task")
            || orchestrator_spawn.contains("role_onboarding_task")
    );
}

#[test]
fn launch_paths_route_through_seam() {
    let call_count = [
        include_str!("spawn.rs"),
        include_str!("../spawn.rs"),
        include_str!("../pty_io/lifecycle.rs"),
    ]
    .into_iter()
    .map(|source| {
        source.matches("launch_agent_in_background(").count()
            + source
                .matches("launch_agent_in_background_with_env(")
                .count()
    })
    .sum::<usize>();

    assert_eq!(call_count, 3);
}

fn make_params(agent: &str, task: Option<&str>) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(agent.to_string()),
        task: task.map(|t| t.to_string()),
        starter_prompt: Default::default(),
        task_id: None,
        task_launch_authority: None,
        plan_task: None,
        agent_name: None,
        args: None,
        cwd: None,
        provisioned_cwd: None,
        title: None,
        title_provenance: None,
        description: None,
        role: None,
        role_profile: None,
        parent_tab_id: None,
        plan_id: None,
        can_spawn: None,
        workspace_id: None,
        force_tab: None,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        skip_permissions: None,
        permission_bypass_authorization: None,
        spawned_by_tab_id: None,
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
        audience: Default::default(),
        manager_bootstrap: None,
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
fn build_agent_command_omits_undeclared_reasoning_effort() {
    let reg = effort_registry();
    let mut params = make_params("tiered", None);
    params.reasoning_effort = Some("ultra".into());
    let cmd = build_agent_command("tiered", &params, None, None, &reg);
    assert!(
        !cmd.contains("--effort"),
        "undeclared effort adds no launch flag"
    );
}

#[test]
fn build_agent_command_omits_effort_for_flat_provider() {
    // Defend direct callers that bypass upstream dispatch validation.
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
        let path = cmd
            .split_once("--append-system-prompt-file '")
            .and_then(|(_, rest)| rest.split_once("' -- "))
            .map(|(path, _)| path)
            .expect("command should expose the task file argument");
        if cfg!(target_os = "macos") {
            assert_ne!(
                std::path::Path::new(path).parent(),
                Some(std::path::Path::new("/tmp")),
                "macOS must use its configured temp directory"
            );
        }
        let content = std::fs::read_to_string(path).expect("temp file should exist");
        assert_eq!(content, task, "temp file should contain the task verbatim");
        std::fs::remove_file(path).expect("temp task file should be removable");
        assert!(!std::path::Path::new(path).exists());
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

#[test]
fn fresh_and_resumed_agents_default_to_provider_permissions() {
    let registry = test_registry();
    let fresh = make_params("claude", None);
    let fresh_command = build_agent_command("claude", &fresh, None, None, &registry);
    assert!(!fresh_command.contains("--dangerously-skip-permissions"));

    let mut resumed = make_params("claude", None);
    resumed.resume_session_id = Some("session-123".into());
    let resume_command = build_agent_command("claude", &resumed, None, None, &registry);
    assert!(!resume_command.contains("--dangerously-skip-permissions"));

    let mut stale_legacy = make_params("claude", None);
    stale_legacy.skip_permissions = Some(true);
    stale_legacy.resume_session_id = Some("legacy-restored-session".into());
    let stale_command = build_agent_command("claude", &stale_legacy, None, None, &registry);
    assert!(!stale_command.contains("--dangerously-skip-permissions"));

    let mut hidden_restored = make_params("claude", None);
    hidden_restored.resume_session_id = Some("hidden-restored-session".into());
    hidden_restored.permission_bypass_authorization =
        crate::service::permission_policy::authorize_permission_bypass(
            &registry,
            "claude",
            true,
            true,
            crate::multiplexer::types::TabAudience::User,
        );
    hidden_restored.skip_permissions = Some(
        hidden_restored
            .permission_bypass_authorization
            .is_some(),
    );
    let hidden_command = build_agent_command("claude", &hidden_restored, None, None, &registry);
    assert!(!hidden_command.contains("--dangerously-skip-permissions"));

    let mut explicit = make_params("claude", None);
    explicit.skip_permissions = Some(true);
    explicit.permission_bypass_authorization =
        crate::service::permission_policy::authorize_permission_bypass(
            &registry,
            "claude",
            true,
            false,
            crate::multiplexer::types::TabAudience::User,
        );
    let explicit_command = build_agent_command("claude", &explicit, None, None, &registry);
    assert_eq!(
        explicit_command.matches("--dangerously-skip-permissions").count(),
        1
    );
    let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(explicit);
    let crate::service::spawn::TabCommand::Agent(agent) = spec.command else {
        panic!("visible provider spawn must remain an agent tab");
    };
    assert!(agent.auto_approve, "tab inspection must expose the active bypass");
}

#[test]
fn hidden_usage_probe_cannot_enable_permission_bypass() {
    let registry = test_registry();
    let mut automation = make_params("claude", None);
    automation.skip_permissions = Some(true);
    automation.audience = crate::multiplexer::types::TabAudience::Automation;
    automation.permission_bypass_authorization =
        crate::service::permission_policy::authorize_permission_bypass(
            &registry,
            "claude",
            true,
            false,
            crate::multiplexer::types::TabAudience::Automation,
        );
    automation.skip_permissions = Some(automation.permission_bypass_authorization.is_some());
    assert!(!automation.permission_bypass_requested("claude"));
    let command = build_agent_command("claude", &automation, None, None, &registry);
    assert!(!command.contains("--dangerously-skip-permissions"));
    let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(automation);
    let crate::service::spawn::TabCommand::Agent(agent) = spec.command else {
        panic!("automation probe must remain an agent tab");
    };
    assert!(!agent.auto_approve, "automation cannot inherit a remembered bypass");
}
