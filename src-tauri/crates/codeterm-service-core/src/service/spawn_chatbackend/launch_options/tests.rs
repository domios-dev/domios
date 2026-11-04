use super::*;

#[test]
fn runtime_workers_default_to_full_access_independently_of_global_choice_and_intent() {
    use crate::service::test_fixture::ServiceFixture;
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};

    let fixture = ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    for (manifest, source) in [
        (
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../plugins/provider-codex/plugin.json"
            )),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../plugins/provider-codex/plugin.js"
            )),
        ),
        (
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../plugins/provider-claude/plugin.json"
            )),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../plugins/provider-claude/plugin.js"
            )),
        ),
    ] {
        let manifest = PluginManifest::from_json_str(manifest).unwrap();
        let provider = manifest.id.clone();
        let vm = std::sync::Arc::new(PluginVm::from_source(&provider, source.into()).unwrap());
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(JsProvider::new(manifest, vm)));
        for choice in [None, Some(false), Some(true)] {
            fixture
                .service
                .ext::<crate::config::ConfigState>()
                .lock()
                .agent
                .full_access = choice;
            for intent in [
                crate::plan::ExecutionIntent::ReadOnly,
                crate::plan::ExecutionIntent::Mutating,
            ] {
                let params = SpawnAgentParams {
                    role: Some("Worker".into()),
                    execution_intent: Some(intent),
                    ..Default::default()
                };
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

#[test]
fn runtime_worker_neighbor_authority_cannot_acquire_an_independent_full_access_grant() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture.set_agent_full_access(true);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let params = SpawnAgentParams {
        role: Some("Worker".into()),
        neighbor_authority: Some(crate::multiplexer::types::NeighborAuthority {
            uid: "opaque-owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        }),
        ..Default::default()
    };
    assert!(!authorize(&fixture.service, "claude", Some(&params))
        .unwrap()
        .permission_bypass_requested("claude"));
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
    }
}

#[test]
fn prepared_options_override_caller_grants_and_preserve_launch_context() {
    let options = prepared_options();
    let config = apply_prepared(
        serde_json::json!({
            "mode": "full-access", "additionalDirectories": ["opaque-caller-root"],
            "networkAccess": true, "model": "opaque-model", "effort": "opaque-effort",
            "systemPrompt": "opaque-prompt"
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
    let mut no_network = options.clone();
    no_network.network_access = None;
    let config = apply_prepared(
        config,
        &options.provider_id,
        Some(&options.binding_nonce),
        Some(&no_network),
    )
    .unwrap();
    assert!(config.get("networkAccess").is_none());
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
