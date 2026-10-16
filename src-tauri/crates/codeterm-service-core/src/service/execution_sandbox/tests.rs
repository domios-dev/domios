use super::*;
use codeterm_plugin::{
    agent_runtime::{RuntimeContext, RuntimeOperation, RuntimeSettings},
    provider::{ExecutionSandboxCapability, ExecutionSandboxMode},
};

fn policy() -> ExecutionSandboxPolicy {
    ExecutionSandboxPolicy {
        intent: ExecutionIntent::ReadOnly,
        cwd: std::env::current_dir().unwrap().to_str().unwrap().into(),
        launch_args: vec!["opaque-arg".into()],
        runtime_mode: Some("opaque-mode".into()),
        writable_roots: Vec::new(),
        environment: Default::default(),
    }
}

#[test]
fn saved_policy_round_trip_rejects_replacement_removal_and_cwd_changes() {
    let original = policy();
    let saved: ExecutionSandboxPolicy =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    assert!(validate_saved(Some(&saved), Some(&original)).is_ok());
    assert!(validate_saved(Some(&saved), None).is_err());
    let mut changed = original.clone();
    changed.intent = ExecutionIntent::Mutating;
    assert!(validate_saved(Some(&saved), Some(&changed)).is_err());
    changed = original.clone();
    changed.cwd.push_str("/foreign");
    assert!(validate_saved(Some(&saved), Some(&changed)).is_err());
    changed = original.clone();
    changed.runtime_mode = Some("full-access".into());
    assert!(validate_saved(Some(&saved), Some(&changed)).is_err());
}

#[test]
fn changed_root_diagnostic_names_the_recorded_or_added_path() {
    let mut saved = policy();
    saved.writable_roots = vec!["recorded-root".into()];
    let mut changed = saved.clone();
    changed.writable_roots = vec!["new-root".into()];
    assert!(validate_saved(Some(&saved), Some(&changed))
        .unwrap_err()
        .contains("recorded-root"));
    changed.writable_roots.insert(0, "recorded-root".into());
    assert!(validate_saved(Some(&saved), Some(&changed))
        .unwrap_err()
        .contains("new-root"));
}

#[test]
fn runtime_policy_rejects_mode_directory_and_cwd_overrides() {
    let policy = policy();
    let settings = RuntimeSettings {
        mode: policy.runtime_mode.clone(),
        network_access: Some(false),
        ..Default::default()
    };
    assert!(validate_runtime_settings(&policy, &settings, &policy.cwd).is_ok());
    assert!(validate_runtime_settings(&policy, &settings, "foreign-cwd").is_err());
    for mode in [
        None,
        Some("full-access".into()),
        Some("auto-accept-edits".into()),
    ] {
        let changed = RuntimeSettings {
            mode,
            ..settings.clone()
        };
        assert!(validate_runtime_settings(&policy, &changed, &policy.cwd).is_err());
    }
    let changed = RuntimeSettings {
        additional_directories: vec![policy.cwd.clone()],
        ..settings
    };
    assert!(validate_runtime_settings(&policy, &changed, &policy.cwd).is_err());
}

#[test]
fn resolver_requires_capability_and_canonicalizes_only_the_lane_root() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-provider").with_execution_sandbox(
            ExecutionSandboxCapability {
                platforms: vec![crate::agent::provider::current_platform_tag().into()],
                read_only: Some(ExecutionSandboxMode {
                    launch_args: vec!["--cwd".into(), "{cwd}".into()],
                    runtime_mode: Some("opaque-mode".into()),
                }),
                ..Default::default()
            },
        ),
    ));
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join(".");
    let resolved = resolve(
        &registry,
        "opaque-provider",
        Some(&ExecutionIntent::ReadOnly),
        cwd.to_str(),
        &[],
        true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(resolved.launch_args[1], resolved.cwd);
    assert_eq!(
        resolved.cwd,
        codeterm_paths::strip_verbatim_prefix(
            directory.path().canonicalize().unwrap().to_str().unwrap()
        )
    );
    assert!(resolve(
        &registry,
        "opaque-provider",
        Some(&ExecutionIntent::ReadOnly),
        Some("relative"),
        &[],
        true
    )
    .is_err());
    assert!(resolve(
        &registry,
        "opaque-provider",
        Some(&ExecutionIntent::ReadOnly),
        cwd.to_str(),
        &["--override".into()],
        true
    )
    .is_err());
    assert!(resolve(
        &registry,
        "missing-provider",
        Some(&ExecutionIntent::ReadOnly),
        cwd.to_str(),
        &[],
        true
    )
    .unwrap_err()
    .contains("Cannot enforce"));
}

#[test]
fn task_bound_workers_require_explicit_execution_intent() {
    use crate::service::orchestrator::TaskLaunchAuthority;
    for binding in 0..3 {
        for intent in [
            None,
            Some(ExecutionIntent::Unspecified),
            Some(ExecutionIntent::ReadOnly),
            Some(ExecutionIntent::Mutating),
        ] {
            let mut params = SpawnAgentParams {
                role: Some("Worker".into()),
                execution_intent: intent.clone(),
                ..Default::default()
            };
            match binding {
                0 => params.plan_task = Some("task-token".into()),
                1 => params.task_id = Some("task-token".into()),
                _ => {
                    params.task_launch_authority = Some(TaskLaunchAuthority {
                        plan_id: "plan-token".into(),
                        task_id: "task-token".into(),
                        attempt_id: "attempt-token".into(),
                        lane_base: None,
                    })
                }
            }
            assert_eq!(
                crate::service::spawn_plan_task::validate_worker_plan_task_spawn(&params).is_ok(),
                matches!(
                    intent,
                    Some(ExecutionIntent::ReadOnly | ExecutionIntent::Mutating)
                )
            );
        }
    }
    assert!(
        crate::service::spawn_plan_task::validate_worker_plan_task_spawn(
            &SpawnAgentParams::default()
        )
        .is_ok()
    );
}

#[test]
fn task_intents_suppress_authorized_global_bypass() {
    let authorization = Some(
        crate::service::permission_policy::PermissionBypassAuthorization::for_test(
            "opaque-provider",
        ),
    );
    for intent in [ExecutionIntent::ReadOnly, ExecutionIntent::Mutating] {
        let params = SpawnAgentParams {
            execution_intent: Some(intent),
            skip_permissions: Some(true),
            permission_bypass_authorization: authorization.clone(),
            ..Default::default()
        };
        assert!(!params.permission_bypass_requested("opaque-provider"));
    }
}

#[test]
fn engine_guard_checks_open_resume_turn_and_controls_against_persisted_policy() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let tab_id = fixture
        .service
        .mux
        .read()
        .all_tabs()
        .next()
        .unwrap()
        .id
        .clone();
    let policy = policy();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .execution_sandbox = Some(policy.clone());
    let settings = RuntimeSettings {
        mode: policy.runtime_mode.clone(),
        network_access: Some(false),
        ..Default::default()
    };
    let context = RuntimeContext {
        tab_id,
        cwd: policy.cwd.clone(),
        settings: settings.clone(),
        ..Default::default()
    };
    let bad = RuntimeSettings {
        mode: Some("full-access".into()),
        ..settings.clone()
    };
    for operation in [
        RuntimeOperation::OpenThread {
            instructions: String::new(),
            settings: bad.clone(),
        },
        RuntimeOperation::ResumeThread {
            thread_id: "opaque-thread".into(),
            instructions: String::new(),
            settings: bad.clone(),
        },
        RuntimeOperation::SubmitTurn {
            thread_id: "opaque-thread".into(),
            active_turn_id: None,
            send_id: "opaque-send".into(),
            input: codeterm_plugin::agent_runtime::TurnInput {
                text: "opaque".into(),
                attachments: Vec::new(),
            },
            settings: bad.clone(),
        },
        RuntimeOperation::Control {
            thread_id: None,
            action: "settings".into(),
            value: serde_json::to_value(&bad).unwrap(),
        },
        RuntimeOperation::Control {
            thread_id: None,
            action: "mode".into(),
            value: "full-access".into(),
        },
    ] {
        assert!(validate_runtime_operation(&fixture.service, &context, &operation).is_err());
    }
    assert!(
        validate_runtime_operation(&fixture.service, &context, &RuntimeOperation::Initialize)
            .is_ok()
    );
}

#[test]
fn restore_reapplies_saved_intent_and_cwd_above_requested_bypass() {
    let policy = policy();
    let params = SpawnAgentParams {
        agent: Some("opaque-provider".into()),
        role: Some("Worker".into()),
        execution_intent: Some(ExecutionIntent::Mutating),
        cwd: Some("foreign-cwd".into()),
        skip_permissions: Some(true),
        ..Default::default()
    };
    let launch = crate::multiplexer::types::RestorableLaunchParameters {
        execution_sandbox: Some(policy.clone()),
        captured: true,
        permission_bypass_requested: true,
        ..Default::default()
    };
    let restored = crate::service::restored_launch::restored_fresh_launch_input(
        &AgentRegistry::new(),
        params,
        launch,
    );
    assert_eq!(
        restored.params.execution_intent,
        Some(ExecutionIntent::ReadOnly)
    );
    assert_eq!(restored.params.cwd, Some(policy.cwd));
    assert_eq!(restored.params.skip_permissions, Some(false));
    assert!(!restored
        .params
        .permission_bypass_requested("opaque-provider"));
    let prompt = restored.launch_task.unwrap();
    assert_eq!(
        prompt
            .matches(crate::prompts::READ_ONLY_WORKER_GUIDANCE.trim())
            .count(),
        1
    );
    assert!(!prompt.contains(crate::prompts::AGENT_PAD_RULE.trim()));
}

#[test]
fn tab_policy_survives_missing_restore_intent_and_refuses_weakening() {
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-provider").with_execution_sandbox(
            ExecutionSandboxCapability {
                platforms: vec![crate::agent::provider::current_platform_tag().into()],
                read_only: Some(ExecutionSandboxMode {
                    launch_args: vec!["opaque-read".into()],
                    runtime_mode: Some("opaque-mode".into()),
                }),
                ..Default::default()
            },
        ),
    ));
    let directory = tempfile::tempdir().unwrap();
    let params = SpawnAgentParams {
        agent: Some("opaque-provider".into()),
        execution_intent: Some(ExecutionIntent::ReadOnly),
        cwd: Some(directory.path().to_str().unwrap().into()),
        ..Default::default()
    };
    let initial = for_launch(&fixture.service, &tab, "opaque-provider", &params, false)
        .unwrap()
        .unwrap();
    let restored = SpawnAgentParams {
        execution_intent: None,
        ..params.clone()
    };
    assert_eq!(
        for_launch(&fixture.service, &tab, "opaque-provider", &restored, true).unwrap(),
        Some(initial.clone())
    );
    let changed = SpawnAgentParams {
        execution_intent: Some(ExecutionIntent::Mutating),
        ..params.clone()
    };
    assert!(for_launch(&fixture.service, &tab, "opaque-provider", &changed, false).is_err());
    let changed = SpawnAgentParams {
        cwd: Some(std::env::current_dir().unwrap().to_str().unwrap().into()),
        ..params
    };
    assert!(for_launch(&fixture.service, &tab, "opaque-provider", &changed, false).is_err());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .restorable_launch
            .execution_sandbox,
        Some(initial)
    );
}

#[test]
fn pty_fresh_and_resume_forward_the_declared_sandbox_without_bypass() {
    let launches = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let resumes = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-provider")
            .with_structured_prompt_launch_log(launches.clone())
            .with_resume_context_log(resumes.clone())
            .with_execution_sandbox(ExecutionSandboxCapability {
                platforms: vec![crate::agent::provider::current_platform_tag().into()],
                read_only: Some(ExecutionSandboxMode {
                    launch_args: vec!["opaque-sandbox".into()],
                    runtime_mode: Some("opaque-mode".into()),
                }),
                ..Default::default()
            }),
    ));
    let directory = tempfile::tempdir().unwrap();
    let params = SpawnAgentParams {
        cwd: Some(directory.path().to_str().unwrap().into()),
        execution_intent: Some(ExecutionIntent::ReadOnly),
        skip_permissions: Some(true),
        permission_bypass_authorization: Some(
            crate::service::permission_policy::PermissionBypassAuthorization::for_test(
                "opaque-provider",
            ),
        ),
        ..Default::default()
    };
    let command = crate::service::orchestrator::build_agent_command(
        "opaque-provider",
        &params,
        None,
        None,
        &registry,
    );
    assert!(!command.is_empty());
    assert_eq!(launches.lock().len(), 1);
    assert!(!launches.lock()[0].skip_permissions);
    assert_eq!(
        launches.lock()[0].args.as_deref().unwrap(),
        &["opaque-sandbox".to_string()]
    );
    let resumed = SpawnAgentParams {
        resume_session_id: Some("opaque-session".into()),
        ..params.clone()
    };
    assert!(!crate::service::orchestrator::build_agent_command(
        "opaque-provider",
        &resumed,
        None,
        None,
        &registry
    )
    .is_empty());
    assert_eq!(resumes.lock().len(), 1);
    assert!(!resumes.lock()[0].skip_permissions);
    assert_eq!(resumes.lock()[0].args, vec!["opaque-sandbox"]);
    let overridden = SpawnAgentParams {
        args: Some(vec!["--override".into()]),
        ..params
    };
    assert!(crate::service::orchestrator::build_agent_command(
        "opaque-provider",
        &overridden,
        None,
        None,
        &registry
    )
    .is_empty());
    assert_eq!(launches.lock().len(), 1);
}
