use super::*;

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
    let registry = registry_with(ModelProvider::new("opencode", &["opencode/big-pickle"]));

    assert!(validate_agent_model(&registry, "opencode", Some("does-not-exist")).is_ok());
    let resolved = resolve_fresh_agent_model_with_notice(
        &registry,
        "opencode",
        Some("does-not-exist".into()),
        None,
    );
    assert_eq!(resolved.model.as_deref(), Some("does-not-exist"));
    assert!(resolved.fallback_notice.is_none());
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
fn free_form_provider_preserves_shortened_unknown_ids() {
    let registry = registry_with(ModelProvider::new(
        "opencode",
        &["opencode/big-pickle", "opencode/ring-2.6-1t"],
    ));

    assert!(validate_agent_model(&registry, "opencode", Some("big-pickle")).is_ok());
    assert_eq!(
        resolve_fresh_agent_model(&registry, "opencode", Some("big-pickle".into()), None)
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
    assert_eq!(data["closest_model_ids"][0], "gpt-6-astra");
    let refused = validation.map_err(|error| error.to_string()).and_then(|_| {
        let clearance = provider_launch_preflight(&registry, "codex-like")
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
            id: "gpt-6-astra".into(),
            display_name: "GPT-6-Astra".into(),
            ..Default::default()
        }],
        Some("gpt-6-astra".into()),
    );

    let error = validate_agent_model(&registry, provider_id, Some("gpt-6-sol"))
        .expect_err("a static seed is not part of a live authoritative catalogue");
    assert!(
        error
            .to_string()
            .ends_with("closest catalogue ids: gpt-6-astra"),
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
        crate::service::spawn::normalize_requested_model("codex", "codex:gpt-6-sol"),
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
