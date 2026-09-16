use super::super::{
    provider_mismatch_error, resolve_dispatch_routing, unqualified_model_error,
};

fn providers() -> Vec<String> {
    vec!["claude".into(), "codex".into(), "opencode".into()]
}

#[test]
fn routing_explicit_agent_wins_and_keeps_model() {
    let r = resolve_dispatch_routing(
        Some("codex".into()),
        Some("gpt-5".into()),
        None,
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("gpt-5"));
}

#[test]
fn routing_explicit_agent_normalizes_matching_qualified_model() {
    let r = resolve_dispatch_routing(
        Some("codex".into()),
        Some("codex:gpt-5.6-sol".into()),
        None,
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("gpt-5.6-sol"));
    assert!(r.qualified_model);
    assert!(r.qualified_provider_mismatch.is_none());
}

#[test]
fn routing_explicit_agent_rejects_unavailable_matching_qualified_task_model() {
    let routing = resolve_dispatch_routing(
        Some("codex".into()),
        None,
        Some("CoDeX:missing-model".into()),
        &providers(),
    );
    assert_eq!(routing.agent.as_deref(), Some("codex"));
    assert_eq!(routing.model.as_deref(), Some("missing-model"));
    assert!(routing.qualified_model);

    let notice = crate::service::spawn::ModelFallbackNotice {
        planned_model: Some("CoDeX:missing-model".into()),
        requested_model: "missing-model".into(),
        provider_resolved_model: "gpt-5.5".into(),
        actual_model: None,
        reason: "requested_model_unavailable".into(),
    };
    let error = unqualified_model_error(&routing, "codex", &notice)
        .expect("explicit qualified unavailable model must be refused");
    assert_eq!(error.code, -32602);
    assert_eq!(error.data.unwrap()["type"], "qualified_model_unavailable");
}

#[test]
fn routing_explicit_agent_rejects_mismatched_qualified_model() {
    let r = resolve_dispatch_routing(
        Some("codex".into()),
        Some("claude:opus".into()),
        None,
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("claude:opus"));
    assert_eq!(
        r.qualified_provider_mismatch
            .as_ref()
            .map(|(provider, _)| provider.as_str()),
        Some("claude")
    );

    let error = provider_mismatch_error(&r).unwrap();
    assert_eq!(error.code, -32602);
    let data = error.data.expect("provider mismatch data");
    assert_eq!(data["type"], "model_provider_mismatch");
    assert_eq!(data["requested_provider"], "claude");
    assert_eq!(data["selected_provider"], "codex");
    assert!(error.message.contains("does not match"));
}

#[test]
fn routing_explicit_agent_preserves_malformed_qualified_models_for_refusal() {
    for model in ["codex:", ":gpt-5.6-sol", "codex"] {
        let r =
            resolve_dispatch_routing(Some("codex".into()), Some(model.into()), None, &providers());
        assert_eq!(r.agent.as_deref(), Some("codex"));
        assert_eq!(r.model.as_deref(), Some(model));
        assert!(!r.qualified_model);
        assert!(r.qualified_provider_mismatch.is_none());
    }
}

#[test]
fn routing_model_naming_a_provider_becomes_that_provider() {
    // This is the exact BUG A repro: `--model codex` must route to the codex
    // provider at its default model, not to the default provider.
    let r = resolve_dispatch_routing(None, Some("codex".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_unknown_model_stays_a_model_for_default_provider() {
    let r = resolve_dispatch_routing(None, Some("opus".into()), None, &providers());
    assert_eq!(r.agent, None);
    assert_eq!(r.model.as_deref(), Some("opus"));
}

#[test]
fn routing_qualified_model_selects_its_registered_provider() {
    let r = resolve_dispatch_routing(None, Some("claude:opus".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("claude"));
    assert_eq!(r.model.as_deref(), Some("opus"));

    let r = resolve_dispatch_routing(None, Some("codex:gpt-5.6-terra".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("gpt-5.6-terra"));
}

#[test]
fn routing_never_silently_substitutes_an_unqualified_model() {
    let routing = resolve_dispatch_routing(None, Some("opus".into()), None, &providers());
    let notice = crate::service::spawn::ModelFallbackNotice {
        planned_model: Some("opus".into()),
        requested_model: "opus".into(),
        provider_resolved_model: "gpt-5.6-terra".into(),
        actual_model: None,
        reason: "requested_model_unavailable".into(),
    };

    let error = unqualified_model_error(&routing, "codex", &notice).unwrap();
    assert_eq!(error.code, -32602);
    assert_eq!(error.data.unwrap()["type"], "unqualified_model_unavailable");

    let explicit = resolve_dispatch_routing(
        Some("codex".into()),
        Some("opus".into()),
        None,
        &providers(),
    );
    let explicit_error = unqualified_model_error(&explicit, "codex", &notice).unwrap();
    assert_eq!(explicit_error.code, -32602);
    assert_eq!(
        explicit_error.data.unwrap()["type"],
        "unqualified_model_unavailable"
    );

    let qualified = resolve_dispatch_routing(None, Some("claude:opus".into()), None, &providers());
    let qualified_error = unqualified_model_error(&qualified, "claude", &notice).unwrap();
    assert_eq!(qualified_error.code, -32602);
    assert_eq!(
        qualified_error.data.unwrap()["type"],
        "qualified_model_unavailable"
    );
}

#[test]
fn routing_falls_back_to_task_model_when_no_rpc_model() {
    let r = resolve_dispatch_routing(None, None, Some("codex".into()), &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_rpc_model_overrides_task_model() {
    let r = resolve_dispatch_routing(
        None,
        Some("opencode".into()),
        Some("codex".into()),
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("opencode"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_empty_inputs_use_defaults() {
    let r = resolve_dispatch_routing(Some("  ".into()), Some("".into()), None, &providers());
    assert_eq!(r.agent, None);
    assert_eq!(r.model, None);
}
