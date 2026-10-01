use super::*;

fn registry_with_models(
    entries: &[(&'static str, &[&'static str])],
) -> crate::agent::registry::AgentRegistry {
    let registry = crate::agent::registry::AgentRegistry::new();
    for (provider_id, model_ids) in entries {
        registry.register_provider(Box::new(ModelProvider::new(*provider_id, *model_ids)));
    }
    registry
}

#[test]
fn validate_agent_model_accepts_advertised_id() {
    let provider = ModelProvider::new(
        "authoritative-catalogue",
        &["anthropic/claude-sonnet-4-6", "opencode/big-pickle"],
    )
    .with_model_catalogue(crate::agent::provider::ModelCatalogueCapability::authoritative());
    let registry = registry_with(provider);

    assert!(validate_agent_model(
        &registry,
        "authoritative-catalogue",
        Some("opencode/big-pickle")
    )
    .is_ok());
    assert!(validate_agent_model(
        &registry,
        "authoritative-catalogue",
        Some("anthropic/claude-sonnet-4-6")
    )
    .is_ok());
}

#[test]
fn free_form_provider_keeps_unknown_model_ids() {
    let provider_id = "free-form-no-live-fixture";
    let registry = registry_with(ModelProvider::new(provider_id, &["opencode/big-pickle"]));

    assert!(validate_agent_model(&registry, provider_id, Some("does-not-exist")).is_ok());
    let resolved = resolve_fresh_agent_model_with_notice(
        &registry,
        provider_id,
        Some("does-not-exist".into()),
        None,
    );
    assert_eq!(resolved.model.as_deref(), Some("does-not-exist"));
    assert_eq!(resolved.provider_resolved_model, None);
    assert!(resolved.fallback_notice.is_none());
}

#[test]
fn free_form_provider_refuses_unknown_ids_when_live_models_are_published() {
    let provider_id = "live-free-form-fixture";
    let registry = registry_with(ModelProvider::new(provider_id, &[]));
    crate::agent::model_catalog::put_live(
        provider_id,
        vec![crate::agent::provider::ModelInfo {
            id: "opencode/deepseek-v4-flash-free".into(),
            display_name: "OpenCode DeepSeek Flash".into(),
            ..Default::default()
        }],
        None,
    );
    let error = validate_agent_model(&registry, provider_id, Some("sonnet"))
        .expect_err("a live provider model list constrains free-form ids");
    assert!(
        error
            .to_string()
            .contains("opencode/deepseek-v4-flash-free"),
        "{error}"
    );
    assert!(validate_agent_model(
        &registry,
        provider_id,
        Some("opencode/deepseek-v4-flash-free")
    )
    .is_ok());
    crate::agent::model_catalog::unregister(provider_id);
}

#[test]
fn spawn_model_validation_refuses_a_foreign_provider_alias() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(ModelProvider::new("claude", &["sonnet", "opus"])));
    registry.register_provider(Box::new(ModelProvider::new("opencode", &[])));

    let error = validate_agent_model(&registry, "opencode", Some("sonnet"))
        .expect_err("Claude alias cannot be passed to a different provider");
    assert!(error.to_string().contains("claude"), "{error}");
    assert!(validate_agent_model(&registry, "claude", Some("sonnet")).is_ok());
}

#[test]
fn claude_alias_never_routes_to_another_provider() {
    let registry = registry_with_models(&[
        ("claude", &["sonnet", "opus"]),
        ("opencode", &["opencode/deepseek-v4-flash-free"]),
    ]);
    let selected = crate::service::spawn::resolve_agent_model_selection(
        &registry,
        None,
        Some("sonnet"),
        "opencode",
    )
    .expect("a unique advertised alias selects its provider");

    assert_eq!(selected.provider_id, "claude");
    assert_eq!(selected.model.as_deref(), Some("sonnet"));
}

#[test]
fn explicit_provider_accepts_a_model_it_shares_with_another_provider() {
    let registry = registry_with_models(&[
        ("provider-x", &["shared-model"]),
        ("provider-y", &["shared-model"]),
    ]);
    let selected = crate::service::spawn::resolve_agent_model_selection(
        &registry,
        Some("provider-x"),
        Some("shared-model"),
        "provider-y",
    )
    .expect("the selected provider advertises the requested id");

    assert_eq!(selected.provider_id, "provider-x");
    assert_eq!(selected.model.as_deref(), Some("shared-model"));
}

#[test]
fn qualified_model_selects_its_provider_and_normalizes_its_id() {
    let registry = registry_with_models(&[("claude", &["opus"]), ("codex", &["gpt-6-luna"])]);
    let selected = crate::service::spawn::resolve_agent_model_selection(
        &registry,
        None,
        Some("codex:gpt-6-luna"),
        "claude",
    )
    .expect("qualified model selects its registered provider");

    assert_eq!(selected.provider_id, "codex");
    assert_eq!(selected.model.as_deref(), Some("gpt-6-luna"));
}

#[test]
fn bare_provider_name_selects_that_providers_default() {
    let registry = registry_with_models(&[("claude", &["opus"]), ("codex", &["gpt-6-luna"])]);
    let selected = crate::service::spawn::resolve_agent_model_selection(
        &registry,
        None,
        Some("codex"),
        "claude",
    )
    .expect("provider name selects its own default model");

    assert_eq!(selected.provider_id, "codex");
    assert_eq!(selected.model, None);
}

#[test]
fn ambiguous_bare_model_requires_an_explicit_provider() {
    let registry = registry_with_models(&[
        ("provider-x", &["shared-model"]),
        ("provider-y", &["shared-model"]),
    ]);
    let error = crate::service::spawn::resolve_agent_model_selection(
        &registry,
        None,
        Some("shared-model"),
        "provider-x",
    )
    .expect_err("an ambiguous id must not fall through to the default provider");

    assert_eq!(
        error.ambiguous_providers,
        vec!["provider-x".to_string(), "provider-y".to_string()]
    );
    assert_eq!(error.rpc_data()["type"], "ambiguous_model");
    assert!(error.to_string().contains("ambiguous model id"), "{error}");
    assert!(error.to_string().contains("pass --agent"), "{error}");
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn spawn_refuses_model_absent_from_free_form_providers_live_list() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        ModelProvider::new("live-spawn-fixture", &[]).with_cli("cmd.exe", "cmd.exe"),
    ));
    crate::agent::model_catalog::put_live(
        "live-spawn-fixture",
        vec![crate::agent::provider::ModelInfo {
            id: "opencode/deepseek-v4-flash-free".into(),
            display_name: "OpenCode DeepSeek Flash".into(),
            ..Default::default()
        }],
        None,
    );
    let before = fixture.service.mux.read().snapshot().tabs.len();
    let params = SpawnAgentParams {
        agent: Some("live-spawn-fixture".into()),
        model: Some("sonnet".into()),
        ..Default::default()
    };
    let error = fixture
        .service
        .spawn_visible_agent_impl(SpawnTabSpec::from_agent_params(params), None)
        .expect_err("spawn must refuse a model missing from the live list");

    assert!(error.contains("opencode/deepseek-v4-flash-free"), "{error}");
    assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
    crate::agent::model_catalog::unregister("live-spawn-fixture");
}

#[test]
fn validate_agent_model_accepts_claude_alias() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    for alias in ["opus", "claude-opus-5", "claude-fable-5-1", "opus[1m]"] {
        assert!(
            validate_agent_model(&registry, "claude", Some(alias)).is_ok(),
            "{alias}"
        );
    }
    assert_eq!(
        resolve_fresh_agent_model_with_notice(&registry, "claude", Some("opus[1m]".into()), None,)
            .model
            .as_deref(),
        Some("opus[1m]")
    );
}

#[test]
fn concrete_claude_model_stays_concrete_when_provider_does_not_offer_alias() {
    let provider_id = "concrete-model-without-alias";
    let registry = registry_with(
        ModelProvider::new(provider_id, &["claude-opus-5"]).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative(),
        ),
    );

    let resolved = resolve_fresh_agent_model_with_notice(
        &registry,
        provider_id,
        Some("claude-opus-5".into()),
        None,
    );

    assert_eq!(resolved.model.as_deref(), Some("claude-opus-5"));
    let launch_args = registry
        .with_provider(provider_id, |provider| {
            provider.model_launch_args(resolved.model.as_deref().expect("resolved model"))
        })
        .expect("fixture provider remains registered");
    assert_eq!(
        launch_args,
        vec!["--model".to_string(), "claude-opus-5".to_string()]
    );
    crate::agent::model_catalog::unregister(provider_id);
}

#[test]
fn restore_with_model_missing_from_authoritative_catalogue_records_notice() {
    let provider_id = "restore-missing-model-fixture";
    let registry = registry_with(
        ModelProvider::new(provider_id, &["provider-default", "other-model"]).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative(),
        ),
    );

    let resumed = resolve_fresh_agent_model_with_notice(
        &registry,
        provider_id,
        Some("removed-model".into()),
        Some("restored-session"),
    );

    assert_eq!(resumed.model.as_deref(), Some("provider-default"));
    assert_eq!(resumed.requested_model.as_deref(), Some("removed-model"));
    let notice = resumed
        .restore_notification
        .as_deref()
        .expect("a removed saved model is notification-worthy");
    assert!(notice.contains("removed-model"), "{notice}");
    assert!(notice.contains("provider-default"), "{notice}");
    let warning = resumed
        .warning
        .expect("a removed saved model must produce a restore notice");
    assert!(warning.contains("removed-model"), "{warning}");
    assert!(warning.contains("provider-default"), "{warning}");
    crate::agent::model_catalog::unregister(provider_id);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn restore_with_undiscovered_live_catalogue_posts_no_notification() {
    let provider_id = "restore-undiscovered-live-catalogue-fixture";
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let registry = registry_with(
        ModelProvider::new(provider_id, &["seed-model"]).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative_live(),
        ),
    );
    let params = SpawnAgentParams {
        agent: Some(provider_id.into()),
        model: Some("saved-model".into()),
        resume_session_id: Some("restore-session".into()),
        ..Default::default()
    };

    crate::service::orchestrator::build_agent_command_for_service(
        &fixture.service,
        &tab_id,
        provider_id,
        &params,
        None,
        None,
        &registry,
    );

    assert!(fixture.service.notif.lock().list(false).is_empty());
    crate::agent::model_catalog::unregister(provider_id);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn restore_with_removed_authoritative_model_posts_notification() {
    let provider_id = "restore-removed-model-notification-fixture";
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let registry = registry_with(
        ModelProvider::new(provider_id, &["provider-default", "other-model"]).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative(),
        ),
    );
    let params = SpawnAgentParams {
        agent: Some(provider_id.into()),
        model: Some("removed-model".into()),
        resume_session_id: Some("restore-session".into()),
        ..Default::default()
    };

    crate::service::orchestrator::build_agent_command_for_service(
        &fixture.service,
        &tab_id,
        provider_id,
        &params,
        None,
        None,
        &registry,
    );

    {
        let store = fixture.service.notif.lock();
        let notifications = store.list(false);
        assert_eq!(notifications.len(), 1);
        assert!(notifications[0].title.contains("restored model"));
        assert!(notifications[0].body.as_deref().is_some_and(|body| {
            body.contains("removed-model") && body.contains("provider-default")
        }));
    }
    crate::agent::model_catalog::unregister(provider_id);
}

#[test]
fn free_form_provider_preserves_shortened_unknown_ids() {
    let provider_id = "free-form-short-id-fixture";
    let registry = registry_with(ModelProvider::new(
        provider_id,
        &["opencode/big-pickle", "opencode/ring-2.6-1t"],
    ));

    assert!(validate_agent_model(&registry, provider_id, Some("big-pickle")).is_ok());
    assert_eq!(
        resolve_fresh_agent_model(&registry, provider_id, Some("big-pickle".into()), None)
            .as_deref(),
        Some("big-pickle")
    );
}

#[test]
fn unknown_model_on_authoritative_catalogue_is_refused_before_spawn() {
    let provider = ModelProvider::new("codex-like", &["gpt-6-astra", "gpt-6-sol"])
        .with_model_catalogue(crate::agent::provider::ModelCatalogueCapability::authoritative());
    let registry = registry_with(provider);
    let (mut mux, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let before = mux.snapshot().tabs.len();

    let validation = validate_agent_model(&registry, "codex-like", Some("gpt-9-bogus"));
    let data = validation
        .as_ref()
        .expect_err("unknown authoritative ids must be refused")
        .rpc_data();
    assert_eq!(data["type"], "invalid_model");
    assert_eq!(data["provider"], "codex-like");
    let valid = data["valid_model_ids"]
        .as_array()
        .expect("valid ids")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(valid, ["gpt-6-astra", "gpt-6-sol"]);
    let refused = validation.map_err(|error| error.to_string()).and_then(|_| {
        let clearance = provider_launch_preflight(&registry, "codex-like", false)
            .map_err(|error| error.to_string())?;
        create_cleared_agent_tab(&mut mux, &workspace_id, &clearance)
            .map_err(|error| error.to_string())
    });
    let error = refused.expect_err("unknown authoritative ids must be refused");
    assert!(error.contains("gpt-9-bogus"), "{error}");
    assert!(error.contains("gpt-6-astra"), "{error}");
    assert!(error.contains("gpt-6-sol"), "{error}");
    assert_eq!(mux.snapshot().tabs.len(), before);
}

#[test]
fn unavailable_authoritative_catalogue_warns_and_preserves_requested_model() {
    let provider = ModelProvider::new("live-catalogue-fixture", &["gpt-6-sol"])
        .with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative_live(),
        );
    let registry = registry_with(provider);
    let requested = "gpt-9-bogus";

    assert!(validate_agent_model(&registry, "live-catalogue-fixture", Some(requested)).is_ok());
    let resolved = resolve_fresh_agent_model_with_notice(
        &registry,
        "live-catalogue-fixture",
        Some(requested.into()),
        None,
    );
    assert_eq!(resolved.model.as_deref(), Some(requested));
    let warning = resolved
        .warning
        .expect("unavailable live catalog must be visible");
    assert!(warning.contains("unchanged"), "{warning}");
    assert!(warning.contains("codeterm agent models"), "{warning}");

    let resumed = resolve_fresh_agent_model_with_notice(
        &registry,
        "live-catalogue-fixture",
        Some(requested.into()),
        Some("session-1"),
    );
    assert_eq!(resumed.model.as_deref(), Some(requested));
    assert!(resumed.warning.is_some());
}

#[test]
fn authoritative_live_catalogue_rejects_seed_only_models() {
    let provider_id = "live-authoritative-fixture";
    let provider = ModelProvider::new(provider_id, &["gpt-6-sol", "gpt-6-astra"])
        .with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative_live(),
        );
    let registry = registry_with(provider);
    crate::agent::model_catalog::put_live(
        provider_id,
        vec![crate::agent::provider::ModelInfo {
            default_reasoning_effort: None,
            id: "gpt-6-astra".into(),
            display_name: "GPT-6-Astra".into(),
            ..Default::default()
        }],
        Some("gpt-6-astra".into()),
    );

    let error = validate_agent_model(&registry, provider_id, Some("gpt-6-sol"))
        .expect_err("a static seed is not part of a live authoritative catalogue");
    assert!(
        error.to_string().ends_with("valid model ids: gpt-6-astra"),
        "{error}"
    );
    crate::agent::model_catalog::unregister(provider_id);
}

#[test]
fn authoritative_ids_accept_matching_provider_prefix() {
    let provider_id = "codex-prefix-fixture";
    let registry = registry_with(
        ModelProvider::new(provider_id, &["gpt-6-sol", "gpt-6-astra"]).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative(),
        ),
    );

    assert!(validate_agent_model(
        &registry,
        provider_id,
        Some("codex-prefix-fixture:gpt-6-sol")
    )
    .is_ok());
    assert_eq!(
        resolve_fresh_agent_model_with_notice(
            &registry,
            provider_id,
            Some("codex-prefix-fixture:gpt-6-sol".into()),
            None,
        )
        .model
        .as_deref(),
        Some("gpt-6-sol")
    );
    assert_eq!(
        crate::service::spawn::spawn_model_catalogue::normalize_requested_model(
            "codex",
            "codex:gpt-6-sol"
        ),
        "gpt-6-sol"
    );
    crate::agent::model_catalog::unregister(provider_id);
}

#[test]
fn authoritative_live_default_warns_when_provider_default_is_stale() {
    let provider_id = "live-default-fallback-fixture";
    let registry = registry_with(
        ModelProvider::new(provider_id, &["seed-default", "live-default"]).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative_live(),
        ),
    );
    crate::agent::model_catalog::put_live(
        provider_id,
        vec![crate::agent::provider::ModelInfo {
            default_reasoning_effort: None,
            id: "live-default".into(),
            display_name: "Live Default".into(),
            ..Default::default()
        }],
        Some("live-default".into()),
    );

    assert!(validate_agent_model(&registry, provider_id, None).is_ok());
    let resolution = resolve_fresh_agent_model_with_notice(&registry, provider_id, None, None);
    assert_eq!(resolution.model.as_deref(), Some("live-default"));
    let warning = resolution.warning.expect("stale defaults must be visible");
    assert!(warning.contains("seed-default"), "{warning}");
    assert!(warning.contains("live-default"), "{warning}");
    crate::agent::model_catalog::unregister(provider_id);
}
#[test]
fn validate_agent_model_passes_through_non_enumerable_provider() {
    let registry = registry_with(ModelProvider::new("dynamic", &[]));

    assert!(validate_agent_model(&registry, "dynamic", Some("anything-goes")).is_ok());
}

#[test]
fn invalid_model_error_lists_every_valid_id_in_a_stable_order() {
    let ids = ["model-a", "model-b", "model-c", "model-d", "model-e"];
    let registry = registry_with(
        ModelProvider::new("full-list-fixture", &ids).with_model_catalogue(
            crate::agent::provider::ModelCatalogueCapability::authoritative(),
        ),
    );

    let listed = |requested: &str| {
        validate_agent_model(&registry, "full-list-fixture", Some(requested))
            .expect_err("unknown ids are refused")
            .valid_model_ids
    };

    let near = listed("model-z");
    let far = listed("qqqqqqqqqqqqqqqq");
    assert_eq!(near, ids);
    assert_eq!(far, near);
}
