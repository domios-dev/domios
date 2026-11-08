use super::*;

fn permission_provider(id: &str, sandboxed: bool) -> crate::agent::test_provider::MockProvider {
    use codeterm_plugin::provider::{ExecutionSandboxCapability, ExecutionSandboxMode};
    let sandbox = if sandboxed {
        ExecutionSandboxCapability {
            platforms: vec!["windows".into(), "linux".into(), "macos".into()],
            read_only: Some(ExecutionSandboxMode {
                launch_args: vec!["--sandbox".into(), "read-only".into()],
                runtime_mode: Some("read-only".into()),
            }),
            mutating: Some(ExecutionSandboxMode {
                launch_args: vec!["--sandbox".into(), "workspace-write".into()],
                runtime_mode: Some("workspace-write".into()),
            }),
            ..Default::default()
        }
    } else {
        ExecutionSandboxCapability {
            unsupported_reason: Some("Fixture provider has no enforcing sandbox".into()),
            ..Default::default()
        }
    };
    let flags = if sandboxed {
        vec![
            "--fixture-bypass-sandbox".into(),
            "--fixture-bypass-trust".into(),
        ]
    } else {
        vec!["--fixture-bypass-permissions".into()]
    };
    crate::agent::test_provider::MockProvider::with_chat_ui(id)
        .with_auto_approve_flags(flags)
        .with_execution_sandbox(sandbox)
}

#[test]
fn runtime_agents_default_to_full_access_independently_of_global_choice_and_intent() {
    use crate::service::test_fixture::ServiceFixture;

    let fixture = ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    for (provider, sandboxed) in [("fixture-enforcing", true), ("fixture-unconfined", false)] {
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(permission_provider(provider, sandboxed)));
        for choice in [None, Some(false), Some(true)] {
            fixture.set_agent_full_access_choice(choice);
            use codeterm_types::agent::role_kind::builtins;
            for (role, profile) in [
                ("Manager", Some(builtins::ORCHESTRATOR)),
                ("Manager", Some(builtins::PLANNER)),
                ("Manager", Some(builtins::CURATOR)),
                ("Manager", Some(builtins::GENERAL_AGENT)),
                ("Worker", None),
                ("Manager", Some("opaque-manager")),
            ] {
                for intent in [
                    crate::plan::ExecutionIntent::ReadOnly,
                    crate::plan::ExecutionIntent::Mutating,
                ] {
                    let params = SpawnAgentParams {
                        role: Some(role.into()),
                        role_profile: profile.map(str::to_owned),
                        execution_intent: Some(intent),
                        ..Default::default()
                    };
                    let pty_authorization = permission_policy::spawn_permission_bypass_for_launch(
                        &fixture.service,
                        &provider,
                        role == "Worker",
                        false,
                        Some(&params),
                    )
                    .unwrap();
                    assert!(pty_authorization.is_some(), "{role} {choice:?} {provider}");
                    let authorized = authorize(&fixture.service, &provider, Some(&params)).unwrap();
                    assert!(authorized.permission_bypass_requested(&provider));
                    assert_eq!(
                        runtime_launch_mode(authorized.permission_bypass_requested(&provider)),
                        "full-access"
                    );
                    assert!(crate::service::execution_sandbox::resolve_requested(
                        &fixture.service.agent_registry,
                        &provider,
                        &authorized,
                        true,
                    )
                    .unwrap()
                    .is_none());
                    let command = crate::service::orchestrator::build_agent_command(
                        &provider,
                        &authorized,
                        None,
                        None,
                        &fixture.service.agent_registry,
                    );
                    assert!(!command.is_empty());
                    let flags = fixture
                        .service
                        .agent_registry
                        .with_provider(&provider, |provider| provider.auto_approve_flags())
                        .unwrap();
                    assert!(!flags.is_empty());
                    for flag in flags {
                        assert!(command.contains(&flag));
                    }
                    assert!(!command.contains("--sandbox"));
                    assert!(!command.contains("workspace-write"));
                    assert!(!command.contains("read-only"));
                    for sandbox in ["read-only", "workspace-write"] {
                        let confined = SpawnAgentParams {
                            sandbox: Some(sandbox.into()),
                            ..params.clone()
                        };
                        let authorized =
                            authorize(&fixture.service, &provider, Some(&confined)).unwrap();
                        assert!(!authorized.permission_bypass_requested(&provider));
                        assert!(permission_policy::spawn_permission_bypass_for_launch(
                            &fixture.service,
                            &provider,
                            role == "Worker",
                            true,
                            Some(&confined),
                        )
                        .unwrap()
                        .is_none());
                    }
                    let refused = SpawnAgentParams {
                        skip_permissions: Some(false),
                        ..params
                    };
                    assert!(!authorize(&fixture.service, &provider, Some(&refused))
                        .unwrap()
                        .permission_bypass_requested(&provider));
                }
            }
        }
    }
}

#[test]
fn runtime_worker_neighbor_authority_cannot_acquire_an_independent_full_access_grant() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture.set_agent_full_access(true);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(permission_provider("fixture-neighbor", false)));
    let params = SpawnAgentParams {
        role: Some("Worker".into()),
        neighbor_authority: Some(crate::multiplexer::types::NeighborAuthority {
            uid: "opaque-owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        }),
        ..Default::default()
    };
    assert!(
        !authorize(&fixture.service, "fixture-neighbor", Some(&params))
            .unwrap()
            .permission_bypass_requested("fixture-neighbor")
    );
}

#[test]
fn permission_bypass_requires_authorization_and_defaults_only_when_unspecified() {
    assert!(!permission_request(Some(false), true));
    assert!(permission_request(Some(true), true));
    assert!(!permission_request(Some(true), false));
    assert!(permission_request(None, true));
    assert!(!permission_request(None, false));
}

fn prepared_options() -> crate::multiplexer::types::RestorableRuntimeLaunchOptions {
    crate::multiplexer::types::RestorableRuntimeLaunchOptions {
        provider_id: "fixture-runtime".into(),
        binding_nonce: "fixture-nonce".into(),
        mode: "auto-accept-edits".into(),
        additional_directories: vec!["opaque-host-root".into()],
        network_access: Some(false),
        full_access_mode: None,
    }
}

#[test]
fn prepared_options_override_caller_grants_and_preserve_launch_context() {
    let options = prepared_options();
    let config = apply_prepared(
        serde_json::json!({
            "mode": "full-access", "additionalDirectories": ["opaque-caller-root"],
            "networkAccess": true, "model": "opaque-model", "effort": "opaque-effort",
            "systemPrompt": "opaque-prompt", "fullAccessMode": "opaque-caller-mode"
        }),
        &options.provider_id,
        Some(&options.binding_nonce),
        Some(&options),
    )
    .unwrap();
    assert_eq!(config["mode"], options.mode);
    assert_eq!(
        config["additionalDirectories"],
        serde_json::json!(options.additional_directories)
    );
    assert_eq!(config["networkAccess"], false);
    assert_eq!(config["model"], "opaque-model");
    assert_eq!(config["effort"], "opaque-effort");
    assert_eq!(config["systemPrompt"], "opaque-prompt");
    assert!(
        config.get("fullAccessMode").is_none(),
        "only the host's prepared full-access launch names an agent mode"
    );
    let mut no_network = options.clone();
    no_network.network_access = None;
    no_network.full_access_mode = Some("opaque-approve-all".into());
    let config = apply_prepared(
        config,
        &options.provider_id,
        Some(&options.binding_nonce),
        Some(&no_network),
    )
    .unwrap();
    assert!(config.get("networkAccess").is_none());
    assert_eq!(config["fullAccessMode"], "opaque-approve-all");
}

#[test]
fn prepared_options_refuse_foreign_provider_or_nonce_before_launch() {
    let options = prepared_options();
    for (provider, nonce) in [
        ("foreign-runtime", Some(options.binding_nonce.as_str())),
        (options.provider_id.as_str(), Some("foreign-nonce")),
        (options.provider_id.as_str(), None),
    ] {
        assert!(apply_prepared(serde_json::json!({}), provider, nonce, Some(&options)).is_err());
    }
}

#[test]
fn service_tier_defaults_follow_capability_and_preserve_explicit_tab_choices() {
    for value in [true, false] {
        let mut config = serde_json::json!({"model":"opaque"});
        apply_service_tier_default(&mut config, true, Some(tier_selection(value)));
        assert_eq!(
            config["serviceTier"],
            serde_json::json!(tier_selection(value))
        );
        assert_eq!(
            crate::service::agent_runtime::model::settings_from_config(&config).service_tier,
            Some(tier_selection(value))
        );
        apply_service_tier_default(&mut config, true, Some(tier_selection(!value)));
        assert_eq!(
            config["serviceTier"],
            serde_json::json!(tier_selection(value))
        );
    }
    let mut unsupported = serde_json::json!({});
    apply_service_tier_default(&mut unsupported, false, Some(tier_selection(true)));
    assert_eq!(unsupported, serde_json::json!({}));
}

fn tier_selection(fast: bool) -> codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
    codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
        id: fast.then(|| "priority".into()),
    }
}
