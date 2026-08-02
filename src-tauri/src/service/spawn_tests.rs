use super::*;
use crate::service::{ServiceContext, SpawnAgentParams};
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn resolve_spawn_workspace_prefers_explicit_workspace_over_parent() {
    let parent_ws = "ws-parent".to_string();
    let explicit_ws = "ws-explicit".to_string();
    let resolved = resolve_spawn_workspace_id(
        Some(explicit_ws.clone()),
        Some("parent-tab"),
        |_| Some(parent_ws.clone()),
        Some("ws-active".into()),
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(resolved, explicit_ws);

    let from_parent = resolve_spawn_workspace_id(
        None,
        Some("parent-tab"),
        |_| Some(parent_ws.clone()),
        Some("ws-active".into()),
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(from_parent, parent_ws);
}

#[test]
fn invalid_role_lead_is_rejected() {
    let err = ServiceContext::parse_role("Lead").unwrap_err();
    assert!(
        err.contains("Orchestrator") && err.contains("Manager") && err.contains("Worker"),
        "error must name valid roles: {err}"
    );
}

#[test]
fn valid_role_worker_and_manager_are_accepted() {
    assert!(ServiceContext::parse_role("Worker").is_ok());
    assert!(ServiceContext::parse_role("Manager").is_ok());
}

pub fn agent_spec(provider: &str) -> SpawnTabSpec {
    SpawnTabSpec {
        visibility: TabVisibility::Visible,
        command: TabCommand::Agent(AgentSpawnSpec {
            provider: provider.into(),
            auto_approve: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn spawn_params(provider: &str) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(provider.into()),
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
        task: None,
        starter_prompt: Default::default(),
        task_id: None,
        task_launch_authority: None,
        plan_task: None,
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
        planned_reasoning_effort: None,
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

pub struct ModelProvider {
    id: &'static str,
    models: Vec<crate::agent::provider::ModelInfo>,
    default_model: Option<String>,
    cli_binary: Option<&'static str>,
    install_command: Option<&'static str>,
}

impl ModelProvider {
    pub fn new(id: &'static str, model_ids: &[&str]) -> Self {
        Self {
            id,
            models: model_ids
                .iter()
                .map(|id| crate::agent::provider::ModelInfo {
                    id: (*id).into(),
                    display_name: (*id).into(),
                    description: None,
                    available: None,
                })
                .collect(),
            default_model: model_ids.first().map(|id| (*id).to_string()),
            cli_binary: None,
            install_command: None,
        }
    }

    pub fn with_cli(mut self, binary: &'static str, install_command: &'static str) -> Self {
        self.cli_binary = Some(binary);
        self.install_command = Some(install_command);
        self
    }
}

impl crate::agent::provider::AgentProvider for ModelProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        self.id
    }

    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        self.id.into()
    }

    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn available_models(&self) -> Vec<crate::agent::provider::ModelInfo> {
        self.models.clone()
    }

    fn default_model(&self) -> Option<String> {
        self.default_model.clone()
    }

    fn cli_binary(&self) -> Option<&str> {
        self.cli_binary
    }

    fn install_command(&self, _: &str) -> Option<String> {
        self.install_command.map(str::to_string)
    }
}

pub fn registry_with(provider: ModelProvider) -> crate::agent::registry::AgentRegistry {
    crate::agent::model_catalog::register(
        provider.id,
        crate::agent::model_catalog::RegisteredDefaults {
            models: provider.models.clone(),
            default: provider.default_model.clone(),
        },
    );
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(provider));
    registry
}

#[test]
fn default_visibility_is_visible() {
    let spec = SpawnTabSpec::default();
    assert_eq!(spec.visibility, TabVisibility::Visible);
}

#[test]
fn default_command_is_shell() {
    let spec = SpawnTabSpec::default();
    assert!(matches!(spec.command, TabCommand::Shell));
}

#[test]
fn validate_agent_model_accepts_advertised_id() {
    let registry = registry_with(ModelProvider::new(
        "opencode",
        &["anthropic/claude-sonnet-4-6", "opencode/big-pickle"],
    ));

    assert!(validate_agent_model(&registry, "opencode", Some("opencode/big-pickle")).is_ok());
    assert!(
        validate_agent_model(&registry, "opencode", Some("anthropic/claude-sonnet-4-6")).is_ok()
    );
}

#[test]
fn validate_agent_model_allows_bogus_id_for_default_fallback() {
    let registry = registry_with(ModelProvider::new("opencode", &["opencode/big-pickle"]));

    assert!(validate_agent_model(&registry, "opencode", Some("does-not-exist")).is_ok());
}

#[test]
fn validate_agent_model_accepts_claude_alias() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    assert!(validate_agent_model(&registry, "claude", Some("opus")).is_ok());
}

#[test]
fn validate_agent_model_allows_unique_prefixed_suffix_for_default_fallback() {
    let registry = registry_with(ModelProvider::new(
        "opencode",
        &["opencode/big-pickle", "opencode/ring-2.6-1t"],
    ));

    assert!(validate_agent_model(&registry, "opencode", Some("big-pickle")).is_ok());
}

#[test]
fn validate_agent_model_passes_through_non_enumerable_provider() {
    let registry = registry_with(ModelProvider::new("dynamic", &[]));

    assert!(validate_agent_model(&registry, "dynamic", Some("anything-goes")).is_ok());
}

#[test]
fn provider_cli_availability_passes_without_cli_dependency() {
    let registry = registry_with(ModelProvider::new("embedded-provider", &[]));

    assert_eq!(
        provider_cli_install_warning(&registry, "embedded-provider").unwrap(),
        None
    );
}

#[test]
fn provider_cli_availability_warns_on_missing_binary_with_install_hint() {
    let registry = registry_with(ModelProvider::new("missing-cli", &[]).with_cli(
        "codeterm-definitely-missing-provider-cli",
        "install missing-cli",
    ));

    let warning = provider_cli_install_warning(&registry, "missing-cli")
        .unwrap()
        .unwrap();
    assert!(warning.contains("Provider not installed"));
    assert!(warning.contains("codeterm-definitely-missing-provider-cli"));
    assert!(warning.contains("install missing-cli"));
}

#[test]
fn provider_cli_availability_rejects_invalid_binary_name() {
    let registry =
        registry_with(ModelProvider::new("bad-cli", &[]).with_cli("bad cli", "install bad-cli"));

    let err = provider_cli_install_warning(&registry, "bad-cli").unwrap_err();
    assert!(err.contains("invalid CLI binary"));
}

#[test]
fn fresh_agent_model_uses_provider_default_when_missing() {
    let registry = registry_with(ModelProvider::new(
        "model-test-default",
        &["model-a", "model-b"],
    ));

    assert_eq!(
        resolve_fresh_agent_model(&registry, "model-test-default", None, None).as_deref(),
        Some("model-a")
    );
}

#[test]
fn fresh_agent_model_keeps_explicit_model() {
    let registry = registry_with(ModelProvider::new(
        "model-test-explicit",
        &["model-a", "model-b"],
    ));

    assert_eq!(
        resolve_fresh_agent_model(
            &registry,
            "model-test-explicit",
            Some(" model-b ".into()),
            None
        )
        .as_deref(),
        Some("model-b")
    );
}

#[test]
fn fresh_agent_model_canonicalizes_display_name() {
    let registry = registry_with(ModelProvider {
        id: "model-test-display",
        models: vec![crate::agent::provider::ModelInfo {
            id: "gpt-5.5".into(),
            display_name: "GPT-5.5".into(),
            description: None,
            available: None,
        }],
        default_model: Some("gpt-5.5".into()),
        cli_binary: None,
        install_command: None,
    });

    assert_eq!(
        resolve_fresh_agent_model(
            &registry,
            "model-test-display",
            Some("GPT-5.5".into()),
            None
        )
        .as_deref(),
        Some("gpt-5.5")
    );
}

#[test]
fn fresh_agent_model_falls_back_to_default_for_unknown_request() {
    let registry = registry_with(ModelProvider::new(
        "model-test-stale",
        &["model-a", "model-b"],
    ));

    assert_eq!(
        resolve_fresh_agent_model(&registry, "model-test-stale", Some("stale".into()), None)
            .as_deref(),
        Some("model-a")
    );
}

#[test]
fn fresh_agent_model_reports_fallback_notice() {
    let registry = registry_with(ModelProvider::new(
        "model-test-fallback-notice",
        &["model-a", "model-b"],
    ));

    let mut resolved = resolve_fresh_agent_model_with_notice(
        &registry,
        "model-test-fallback-notice",
        Some("stale".into()),
        None,
    );
    attach_planned_model(&mut resolved, Some("codex/model-planned"));

    assert_eq!(resolved.model.as_deref(), Some("model-a"));
    assert_eq!(resolved.requested_model.as_deref(), Some("stale"));
    assert_eq!(
        resolved.fallback_notice,
        Some(ModelFallbackNotice {
            planned_model: Some("codex/model-planned".into()),
            requested_model: "stale".into(),
            provider_resolved_model: "model-a".into(),
            actual_model: None,
            reason: "requested_model_unavailable".into(),
        })
    );
    assert_eq!(
        model_fallback_notice_title(&resolved.fallback_notice.unwrap()),
        "model stale unavailable -> using model-a"
    );
}

#[test]
fn background_model_ignores_a_model_from_another_provider() {
    use codeterm_mem::worker_agent::BackgroundModelPort;

    let provider_id = "background-model-test";
    crate::agent::model_catalog::register(
        provider_id,
        crate::agent::model_catalog::RegisteredDefaults {
            models: vec![crate::agent::provider::ModelInfo {
                id: "model-a".into(),
                display_name: "Model A".into(),
                description: None,
                available: Some(true),
            }],
            default: Some("model-a".into()),
        },
    );
    assert_eq!(
        CatalogBackgroundModelPort.resolve(provider_id, "haiku"),
        (String::new(), true)
    );
    assert_eq!(
        CatalogBackgroundModelPort.resolve(provider_id, "model-a"),
        ("model-a".into(), false)
    );
}

/// Per-provider resolution for all four worker-capable providers, using
/// model ids mirroring their real plugin/native catalogs (`plugins/provider-
/// {codex,cursor,opencode}/plugin.json`, and the claude short-alias set).
/// Guards `CatalogBackgroundModelPort` (and therefore every memory
/// background-worker call site that now shares it) against a regression that
/// would let one provider's model id leak into another's launch args.
#[test]
fn background_model_resolves_correctly_for_every_installed_provider() {
    use crate::agent::model_catalog::{register, RegisteredDefaults};
    use crate::agent::provider::ModelInfo;
    use codeterm_mem::worker_agent::BackgroundModelPort;

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: id.into(),
            description: None,
            available: Some(true),
        }
    }

    register(
        "claude-provider-test",
        RegisteredDefaults {
            models: vec![model("opus"), model("sonnet"), model("haiku")],
            default: Some("sonnet".into()),
        },
    );
    register(
        "codex-provider-test",
        RegisteredDefaults {
            models: vec![model("gpt-5.5"), model("gpt-5.4-mini")],
            default: Some("gpt-5.5".into()),
        },
    );
    register(
        "cursor-provider-test",
        RegisteredDefaults {
            models: vec![
                model("auto"),
                model("composer-2.5"),
                model("claude-4.6-sonnet-medium"),
            ],
            default: Some("auto".into()),
        },
    );
    register(
        "opencode-provider-test",
        RegisteredDefaults {
            models: vec![
                model("anthropic/claude-opus-4-8"),
                model("anthropic/claude-sonnet-4-6"),
            ],
            default: Some("anthropic/claude-opus-4-8".into()),
        },
    );

    let cases: &[(&str, &str, &str)] = &[
        ("claude-provider-test", "opus", "opus"),
        ("codex-provider-test", "gpt-5.4-mini", "gpt-5.4-mini"),
        ("cursor-provider-test", "composer-2.5", "composer-2.5"),
        (
            "opencode-provider-test",
            "anthropic/claude-sonnet-4-6",
            "anthropic/claude-sonnet-4-6",
        ),
    ];
    for (provider_id, requested, expected) in cases {
        let (resolved, fell_back) = CatalogBackgroundModelPort.resolve(provider_id, requested);
        assert_eq!(resolved, *expected);
        assert!(!fell_back);
    }

    // Cross-provider: codex's model id must not resolve against cursor's
    // catalog (and vice versa) — proves no provider-id branching papers over
    // an actual catalog mismatch.
    let (resolved, fell_back) =
        CatalogBackgroundModelPort.resolve("cursor-provider-test", "gpt-5.5");
    assert_eq!(resolved, "");
    assert!(fell_back);
}

#[test]
fn resume_agent_model_does_not_inject_default() {
    let registry = registry_with(ModelProvider::new(
        "model-test-resume",
        &["model-a", "model-b"],
    ));

    assert_eq!(
        resolve_fresh_agent_model(&registry, "model-test-resume", None, Some("sid-1")),
        None
    );
}

#[test]
fn agent_auto_approve_defaults_false_via_serde() {
    let json = r#"{"provider": "claude"}"#;
    let agent: AgentSpawnSpec = serde_json::from_str(json).unwrap();
    assert!(!agent.auto_approve);
}

#[test]
fn agent_auto_approve_defaults_false_via_struct() {
    let agent = AgentSpawnSpec {
        provider: "claude".into(),
        auto_approve: false,
        ..Default::default()
    };
    assert!(!agent.auto_approve);
}

#[test]
fn shell_spec_roundtrips_json() {
    let spec = SpawnTabSpec {
        title: Some("scratch".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: SpawnTabSpec = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.command, TabCommand::Shell));
    assert_eq!(back.title.as_deref(), Some("scratch"));
}

#[test]
fn agent_spec_roundtrips_json() {
    let spec = SpawnTabSpec {
        visibility: TabVisibility::Hidden,
        command: TabCommand::Agent(AgentSpawnSpec {
            provider: "claude".into(),
            planned_model: None,
            model: Some("opus".into()),
            task: Some("hello".into()),
            role: Some("Worker".into()),
            can_spawn: Some(false),
            auto_approve: true,
            worker_id: Some("w-123".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: SpawnTabSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.visibility, TabVisibility::Hidden);
    match back.command {
        TabCommand::Agent(a) => {
            assert_eq!(a.provider, "claude");
            assert_eq!(a.model.as_deref(), Some("opus"));
            assert_eq!(a.task.as_deref(), Some("hello"));
            assert_eq!(a.role.as_deref(), Some("Worker"));
            assert_eq!(a.worker_id.as_deref(), Some("w-123"));
            assert!(a.auto_approve);
        }
        _ => panic!("expected Agent"),
    }
}

#[test]
fn from_agent_params_carries_model_into_spec() {
    let params = SpawnAgentParams {
        agent: Some("codex".into()),
        agent_name: None,
        args: None,
        cwd: None,
        provisioned_cwd: None,
        title: None,
        title_provenance: None,
        description: Some("triage failing snapshot".into()),
        role: None,
        role_profile: None,
        parent_tab_id: None,
        plan_id: None,
        can_spawn: None,
        task: None,
        starter_prompt: Default::default(),
        task_id: None,
        task_launch_authority: None,
        plan_task: None,
        workspace_id: None,
        force_tab: None,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        skip_permissions: None,
        permission_bypass_authorization: None,
        spawned_by_tab_id: None,
        model: Some("gpt-5.4-mini".into()),
        planned_model: None,
        planned_reasoning_effort: None,
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
    };
    let spec = SpawnTabSpec::from_agent_params(params);
    assert_eq!(spec.description.as_deref(), Some("triage failing snapshot"));
    match spec.command {
        TabCommand::Agent(a) => assert_eq!(a.model.as_deref(), Some("gpt-5.4-mini")),
        _ => panic!("expected Agent"),
    }
}

#[test]
fn to_agent_params_carries_model_as_dedicated_field() {
    // provider via `model_launch_args` to render the correct flag syntax (`-m X` for codex/opencode, `--model X` for claude).
    let mut spec = agent_spec("claude");
    if let TabCommand::Agent(ref mut a) = spec.command {
        a.model = Some("opus".into());
        a.task = Some("count files".into());
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.agent.as_deref(), Some("claude"));
    assert_eq!(params.task.as_deref(), Some("count files"));
    assert_eq!(params.model.as_deref(), Some("opus"));
    let args = params.args.unwrap_or_default();
    assert!(
        !args.iter().any(|a| a == "--model"),
        "model must not leak into args"
    );
}

#[test]
fn planned_effort_survives_host_owned_params_spec_pipeline() {
    let params = SpawnAgentParams {
        agent: Some("claude".into()),
        planned_reasoning_effort: Some("high".into()),
        reasoning_effort: Some("medium".into()),
        ..Default::default()
    };
    let spec = SpawnTabSpec::from_agent_params(params);
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.planned_reasoning_effort.as_deref(), Some("high"));
    assert_eq!(params.reasoning_effort.as_deref(), Some("medium"));
}

#[test]
fn generic_spawn_json_cannot_claim_planned_effort() {
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "planned_reasoning_effort": "max",
        "reasoning_effort": "high"
    }))
    .unwrap();
    assert_eq!(params.planned_reasoning_effort, None);
    assert_eq!(params.reasoning_effort.as_deref(), Some("high"));
}

#[test]
fn to_agent_params_passes_role_and_report_target() {
    let mut spec = agent_spec("claude");
    spec.parent_tab_id = Some("parent-1".into());
    if let TabCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Worker".into());
        a.can_spawn = Some(true);
        a.report_target_tab_id = Some("cron-host".into());
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.role.as_deref(), Some("Worker"));
    assert_eq!(params.parent_tab_id.as_deref(), Some("parent-1"));
    assert_eq!(params.can_spawn, Some(true));
    assert_eq!(params.report_target_tab_id.as_deref(), Some("cron-host"));
}

#[test]
fn to_agent_params_fails_on_shell_spec() {
    let spec = SpawnTabSpec::default();
    let err = spec.to_agent_params().unwrap_err();
    assert!(err.contains("Shell"));
}

#[test]
fn can_spawn_none_round_trips_through_params() {
    // (Orchestrator=true, Worker=false).
    let mut spec = agent_spec("claude");
    if let TabCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Orchestrator".into());
        a.can_spawn = None;
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.can_spawn, None);
}

#[test]
fn can_spawn_some_false_explicitly_overrides() {
    let mut spec = agent_spec("claude");
    if let TabCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Worker".into());
        a.can_spawn = Some(false);
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.can_spawn, Some(false));
}

#[test]
fn from_agent_params_preserves_can_spawn_option() {
    // --can-spawn=b explicitly". The spec must NOT collapse None → Some(false) (the pre-fix-#42 behavior that clobbered Orchestrator default to false on every spawn).
    let mut p = SpawnAgentParams {
        agent: Some("claude".into()),
        agent_name: None,
        args: None,
        cwd: None,
        provisioned_cwd: None,
        title: None,
        title_provenance: None,
        description: None,
        role: Some("Orchestrator".into()),
        role_profile: None,
        parent_tab_id: None,
        plan_id: None,
        can_spawn: None,
        task: None,
        starter_prompt: Default::default(),
        task_id: None,
        task_launch_authority: None,
        plan_task: None,
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
        planned_reasoning_effort: None,
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
    };
    let spec = SpawnTabSpec::from_agent_params(p.clone());
    match spec.command {
        TabCommand::Agent(a) => assert_eq!(a.can_spawn, None),
        _ => panic!("expected Agent"),
    }

    p.can_spawn = Some(true);
    let spec = SpawnTabSpec::from_agent_params(p);
    match spec.command {
        TabCommand::Agent(a) => assert_eq!(a.can_spawn, Some(true)),
        _ => panic!("expected Agent"),
    }
}

// ── activate / focus-on-spawn ─────────────────────────────────────

#[test]
fn from_agent_params_carries_activate_into_spec() {
    let mut p = SpawnAgentParams {
        agent: Some("claude".into()),
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
        task: None,
        starter_prompt: Default::default(),
        task_id: None,
        task_launch_authority: None,
        plan_task: None,
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
        planned_reasoning_effort: None,
        reasoning_effort: None,
        system_prompt: None,
        preset: None,
        resume_session_id: None,
        charter: None,
        tick_interval_secs: None,
        nudge_cooldown_secs: None,
        max_actions_per_tick: None,
        activate: Some(true),
        audience: Default::default(),
        manager_bootstrap: None,
        structured_prompt_server: None,
    };
    let spec = SpawnTabSpec::from_agent_params(p.clone());
    match spec.command {
        TabCommand::Agent(a) => assert_eq!(a.activate, Some(true)),
        _ => panic!("expected Agent"),
    }

    p.activate = None;
    let spec = SpawnTabSpec::from_agent_params(p);
    match spec.command {
        TabCommand::Agent(a) => assert_eq!(a.activate, None),
        _ => panic!("expected Agent"),
    }
}

#[test]
fn to_agent_params_round_trips_activate() {
    let mut spec = agent_spec("claude");
    if let TabCommand::Agent(ref mut a) = spec.command {
        a.activate = Some(true);
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.activate, Some(true));
}

#[test]
fn spawn_agent_params_from_json_parses_activate_true() {
    let json = serde_json::json!({"activate": true});
    let p = SpawnAgentParams::from_json(&json).unwrap();
    assert_eq!(p.activate, Some(true));
}

#[test]
fn spawn_agent_params_from_json_no_activate_means_none() {
    // Regression for the focus-steal bug: CLI / API spawns that don't pass activate must end up as None (default false) — they no longer steal focus just because they don't set a role.
    let json = serde_json::json!({"agent": "claude", "task": "do work"});
    let p = SpawnAgentParams::from_json(&json).unwrap();
    assert!(p.activate.is_none());
    assert!(p.role.is_none(), "no role + no activate is the bug case");
}

// Activation checks are centralized on `activate.unwrap_or(false)`.
// Tests mirror the predicate to prevent future focus-steal regressions from default changes.

fn would_activate(spec: &AgentSpawnSpec) -> bool {
    spec.activate.unwrap_or(false)
}

#[test]
fn spawn_with_activate_true_sets_active_tab() {
    let spec = AgentSpawnSpec {
        provider: "claude".into(),
        activate: Some(true),
        ..Default::default()
    };
    assert!(would_activate(&spec));
}

#[test]
fn spawn_with_activate_false_does_not_steal_focus() {
    let spec = AgentSpawnSpec {
        provider: "claude".into(),
        activate: Some(false),
        ..Default::default()
    };
    assert!(!would_activate(&spec));
}

#[test]
fn spawn_with_no_role_and_no_activate_does_not_steal_focus() {
    // Regression: pre-fix the predicate was `role.is_none()`, so any CLI/API/cron/bench spawn (which legitimately has no role) stole user focus.
    // With opt-in activate, role=None + activate=None must leave focus untouched.
    let spec = AgentSpawnSpec {
        provider: "claude".into(),
        role: None,
        activate: None,
        ..Default::default()
    };
    assert!(!would_activate(&spec));
}

#[test]
fn chat_default_on_capable_visible_agent_sets_chat_view() {
    assert!(should_default_agent_tab_to_chat(
        TabVisibility::Visible,
        true,
    ));
}

#[test]
fn chat_default_on_non_capable_visible_agent_stays_terminal() {
    assert!(!should_default_agent_tab_to_chat(
        TabVisibility::Visible,
        false,
    ));
}

#[test]
fn chat_capable_visible_agent_always_defaults_to_chat() {
    assert!(should_default_agent_tab_to_chat(
        TabVisibility::Visible,
        true,
    ));
}

#[test]
fn chat_default_on_hidden_agent_stays_terminal() {
    assert!(!should_default_agent_tab_to_chat(
        TabVisibility::Hidden,
        true,
    ));
}

#[test]
fn chatbackend_tab_command_deserializes() {
    // plugin id and opaque config (B1.3 #1/#6).
    let json = r#"{"kind":"chat_backend","plugin_id":"local-shell","config":{"model":"qwen"}}"#;
    let cmd: TabCommand = serde_json::from_str(json).unwrap();
    match cmd {
        TabCommand::ChatBackend {
            plugin_id, config, ..
        } => {
            assert_eq!(plugin_id, "local-shell");
            assert_eq!(config["model"], "qwen");
        }
        _ => panic!("expected ChatBackend variant"),
    }
}

#[test]
fn chatbackend_tab_command_config_defaults_to_null() {
    let json = r#"{"kind":"chat_backend","plugin_id":"local-shell"}"#;
    let cmd: TabCommand = serde_json::from_str(json).unwrap();
    assert!(matches!(cmd, TabCommand::ChatBackend { config, .. } if config.is_null()));
}
