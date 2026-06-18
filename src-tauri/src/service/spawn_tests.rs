use super::*;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider};
use crate::service::{ServiceContext, SpawnAgentParams};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn invalid_role_lead_is_rejected() {
    let err = ServiceContext::parse_role("Lead").unwrap_err();
    assert!(
        err.contains("Orchestrator") && err.contains("Worker"),
        "error must name valid roles: {err}"
    );
}

#[test]
fn valid_role_worker_is_accepted() {
    assert!(ServiceContext::parse_role("Worker").is_ok());
}

fn agent_spec(provider: &str) -> SpawnPaneSpec {
    SpawnPaneSpec {
        visibility: PaneVisibility::Visible,
        command: PaneCommand::Agent(AgentSpawnSpec {
            provider: provider.into(),
            auto_approve: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn spawn_params(provider: &str) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(provider.into()),
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        system_prompt: None,
        preset: None,
        resume_session_id: None,
        activate: None,
    }
}

struct ModelProvider {
    id: &'static str,
    models: Vec<crate::agent::provider::ModelInfo>,
    default_model: Option<String>,
    cli_binary: Option<&'static str>,
    install_command: Option<&'static str>,
}

impl ModelProvider {
    fn new(id: &'static str, model_ids: &[&str]) -> Self {
        Self {
            id,
            models: model_ids
                .iter()
                .map(|id| crate::agent::provider::ModelInfo {
                    id: (*id).into(),
                    display_name: (*id).into(),
                    description: None,
                })
                .collect(),
            default_model: model_ids.first().map(|id| (*id).to_string()),
            cli_binary: None,
            install_command: None,
        }
    }

    fn with_cli(mut self, binary: &'static str, install_command: &'static str) -> Self {
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

fn registry_with(provider: ModelProvider) -> crate::agent::registry::AgentRegistry {
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
    let spec = SpawnPaneSpec::default();
    assert_eq!(spec.visibility, PaneVisibility::Visible);
}

#[test]
fn default_command_is_shell() {
    let spec = SpawnPaneSpec::default();
    assert!(matches!(spec.command, PaneCommand::Shell));
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

    let resolved = resolve_fresh_agent_model_with_notice(
        &registry,
        "model-test-fallback-notice",
        Some("stale".into()),
        None,
    );

    assert_eq!(resolved.model.as_deref(), Some("model-a"));
    assert_eq!(
        resolved.fallback_notice,
        Some(ModelFallbackNotice {
            requested: "stale".into(),
            using: "model-a".into(),
        })
    );
    assert_eq!(
        model_fallback_notice_title(&resolved.fallback_notice.unwrap()),
        "model stale unavailable -> using model-a"
    );
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
fn agent_auto_approve_defaults_true_via_serde() {
    let json = r#"{"provider": "claude"}"#;
    let agent: AgentSpawnSpec = serde_json::from_str(json).unwrap();
    assert!(agent.auto_approve);
}

#[test]
fn agent_auto_approve_defaults_true_via_struct() {
    let agent = AgentSpawnSpec {
        provider: "claude".into(),
        auto_approve: default_true(),
        ..Default::default()
    };
    assert!(agent.auto_approve);
}

#[test]
fn shell_spec_roundtrips_json() {
    let spec = SpawnPaneSpec {
        title: Some("scratch".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.command, PaneCommand::Shell));
    assert_eq!(back.title.as_deref(), Some("scratch"));
}

#[test]
fn agent_spec_roundtrips_json() {
    let spec = SpawnPaneSpec {
        visibility: PaneVisibility::Hidden,
        command: PaneCommand::Agent(AgentSpawnSpec {
            provider: "claude".into(),
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
    let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.visibility, PaneVisibility::Hidden);
    match back.command {
        PaneCommand::Agent(a) => {
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
        title: None,
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: Some("gpt-5.4-mini".into()),
        system_prompt: None,
        preset: None,
        resume_session_id: None,
        activate: None,
    };
    let spec = SpawnPaneSpec::from_agent_params(params);
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.model.as_deref(), Some("gpt-5.4-mini")),
        _ => panic!("expected Agent"),
    }
}

#[test]
fn to_agent_params_carries_model_as_dedicated_field() {
    // Model travels as a dedicated `model` field through SpawnAgentParams, not as `--model X` baked into args. `build_agent_command` then asks the
    // provider via `model_launch_args` to render the correct flag syntax (`-m X` for codex/opencode, `--model X` for claude).
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
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
fn to_agent_params_passes_role_and_report_target() {
    let mut spec = agent_spec("claude");
    spec.parent_pane_id = Some("parent-1".into());
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Worker".into());
        a.can_spawn = Some(true);
        a.report_target_pane_id = Some("cron-host".into());
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.role.as_deref(), Some("Worker"));
    assert_eq!(params.parent_pane_id.as_deref(), Some("parent-1"));
    assert_eq!(params.can_spawn, Some(true));
    assert_eq!(params.report_target_pane_id.as_deref(), Some("cron-host"));
}

#[test]
fn to_agent_params_fails_on_shell_spec() {
    let spec = SpawnPaneSpec::default();
    let err = spec.to_agent_params().unwrap_err();
    assert!(err.contains("Shell"));
}

#[test]
fn can_spawn_none_round_trips_through_params() {
    // Caller didn't pass --can-spawn → spec.can_spawn=None → params.can_spawn=None. This tells set_pane_role to keep the role's default
    // (Orchestrator=true, Worker=false).
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Orchestrator".into());
        a.can_spawn = None;
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.can_spawn, None);
}

#[test]
fn can_spawn_some_false_explicitly_overrides() {
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Worker".into());
        a.can_spawn = Some(false);
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.can_spawn, Some(false));
}

#[test]
fn from_agent_params_preserves_can_spawn_option() {
    // CLI/REST/socket pass `can_spawn: Option<bool>` straight through — None means "don't override the role's default", Some(b) means "user said
    // --can-spawn=b explicitly". The spec must NOT collapse None → Some(false) (the pre-fix-#42 behavior that clobbered Orchestrator default to false on every spawn).
    let mut p = SpawnAgentParams {
        agent: Some("claude".into()),
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        role: Some("Orchestrator".into()),
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        system_prompt: None,
        preset: None,
        resume_session_id: None,
        activate: None,
    };
    let spec = SpawnPaneSpec::from_agent_params(p.clone());
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.can_spawn, None),
        _ => panic!("expected Agent"),
    }

    p.can_spawn = Some(true);
    let spec = SpawnPaneSpec::from_agent_params(p);
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.can_spawn, Some(true)),
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
        title: None,
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        system_prompt: None,
        preset: None,
        resume_session_id: None,
        activate: Some(true),
    };
    let spec = SpawnPaneSpec::from_agent_params(p.clone());
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.activate, Some(true)),
        _ => panic!("expected Agent"),
    }

    p.activate = None;
    let spec = SpawnPaneSpec::from_agent_params(p);
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.activate, None),
        _ => panic!("expected Agent"),
    }
}

#[test]
fn to_agent_params_round_trips_activate() {
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
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
    assert!(should_default_agent_pane_to_chat(
        true,
        PaneVisibility::Visible,
        true,
    ));
}

#[test]
fn chat_default_on_non_capable_visible_agent_stays_terminal() {
    assert!(!should_default_agent_pane_to_chat(
        true,
        PaneVisibility::Visible,
        false,
    ));
}

#[test]
fn chat_default_off_capable_visible_agent_stays_terminal() {
    assert!(!should_default_agent_pane_to_chat(
        false,
        PaneVisibility::Visible,
        true,
    ));
}

#[test]
fn chat_default_on_hidden_agent_stays_terminal() {
    assert!(!should_default_agent_pane_to_chat(
        true,
        PaneVisibility::Hidden,
        true,
    ));
}

#[test]
fn chatbackend_pane_command_deserializes() {
    // The spawn API contract for a chatBackend pane: kind "chat_backend" with a
    // plugin id and opaque config (B1.3 #1/#6).
    let json = r#"{"kind":"chat_backend","plugin_id":"local-shell","config":{"model":"qwen"}}"#;
    let cmd: PaneCommand = serde_json::from_str(json).unwrap();
    match cmd {
        PaneCommand::ChatBackend { plugin_id, config } => {
            assert_eq!(plugin_id, "local-shell");
            assert_eq!(config["model"], "qwen");
        }
        _ => panic!("expected ChatBackend variant"),
    }
}

#[test]
fn chatbackend_pane_command_config_defaults_to_null() {
    let json = r#"{"kind":"chat_backend","plugin_id":"local-shell"}"#;
    let cmd: PaneCommand = serde_json::from_str(json).unwrap();
    assert!(matches!(cmd, PaneCommand::ChatBackend { config, .. } if config.is_null()));
}

struct SpawnRouteShell {
    id: &'static str,
}

impl ChatBackendProvider for SpawnRouteShell {
    fn id(&self) -> &str {
        self.id
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn poll(&self, _: &str) -> Result<Vec<crate::agent::chat_types::ChatMsg>, ChatBackendError> {
        Ok(Vec::new())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn spawn_agent_params_for_registered_chatbackend_routes_to_chatbackend_config() {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register(
        "local-shell",
        Arc::new(SpawnRouteShell { id: "local-shell" }),
    );

    let spec = SpawnPaneSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("local-shell".into()),
            system_prompt: Some("You are terse.".into()),
            preset: Some("ops".into()),
            model: Some("qwen".into()),
            ..spawn_params("local-shell")
        },
    );

    match spec.command {
        PaneCommand::ChatBackend { plugin_id, config } => {
            assert_eq!(plugin_id, "local-shell");
            assert_eq!(config["systemPrompt"], "You are terse.");
            assert_eq!(config["preset"], "ops");
            assert_eq!(config["model"], "qwen");
        }
        other => panic!("expected chatBackend command, got {other:?}"),
    }
}

struct BackendOnlyShell;

impl ChatBackendProvider for BackendOnlyShell {
    fn id(&self) -> &str {
        "npc"
    }

    // spawnable() defaults to false — a bare `chatBackend: true` backend.

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn poll(&self, _: &str) -> Result<Vec<crate::agent::chat_types::ChatMsg>, ChatBackendError> {
        Ok(Vec::new())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn spawn_agent_params_for_non_spawnable_chatbackend_does_not_route_to_chatbackend() {
    // A registered-but-not-spawnable backend (bare `chatBackend: true`, e.g. the
    // NPC plugin) must NOT be opened as a ChatBackend pane. It falls through to
    // the Agent path, where validation rejects it as not user-spawnable.
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register("npc", Arc::new(BackendOnlyShell));

    let spec = SpawnPaneSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("npc".into()),
            ..spawn_params("npc")
        },
    );

    match spec.command {
        PaneCommand::Agent(agent) => assert_eq!(agent.provider, "npc"),
        other => panic!("non-spawnable backend must not route to ChatBackend, got {other:?}"),
    }
}

#[test]
fn spawn_agent_params_for_non_chatbackend_keeps_agent_path() {
    let registry = crate::chatbackend::ChatBackendRegistry::new();

    let spec = SpawnPaneSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("pty-agent".into()),
            system_prompt: Some("ignored by PTY providers".into()),
            preset: Some("ignored".into()),
            ..spawn_params("pty-agent")
        },
    );

    match spec.command {
        PaneCommand::Agent(agent) => {
            assert_eq!(agent.provider, "pty-agent");
            assert_eq!(agent.model, None);
        }
        other => panic!("expected PTY agent command, got {other:?}"),
    }
}
