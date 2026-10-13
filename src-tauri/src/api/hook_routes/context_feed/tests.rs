use super::*;

fn usage(used: u64, window: u64) -> TurnContext {
    TurnContext {
        used_tokens: used,
        window_tokens: window,
        cost_usd: None,
        source: "claude-jsonl".into(),
    }
}

#[test]
fn claude_context_feed_merges_authoritative_window_without_adding_turn_totals() {
    let mut previous = usage(100, 200000);
    previous.cost_usd = Some(1.5);
    let context = merge(Some(&previous), usage(25, 0), None).unwrap();
    assert_eq!(context.used_tokens, 25);
    assert_eq!(context.window_tokens, 200000);
    assert_eq!(context.cost_usd, Some(1.5));
    assert_eq!(
        merge(None, usage(25, 0), Some(1000000))
            .unwrap()
            .window_tokens,
        1000000
    );
    assert_eq!(merge(None, usage(25, 0), None).unwrap().window_tokens, 0);
    assert!(merge(None, usage(10_000_001, 0), None).is_none());
}

#[test]
fn claude_signals_compaction_resets_context_and_preserves_known_window_and_cost() {
    use codeterm_plugin::agent_claude::provider::ClaudeProvider;
    use codeterm_plugin::provider::AgentProvider;

    let previous = TurnContext {
        used_tokens: 170000,
        window_tokens: 1000000,
        cost_usd: Some(1.5),
        source: "claude-statusline".into(),
    };
    let provider = ClaudeProvider::new();
    let incoming = provider
        .parse_context_usage(&json!({
            "type":"system","subtype":"compact_boundary",
            "compactMetadata":{"preTokens":170000,"postTokens":25000}
        }))
        .unwrap();
    let merged = merge(Some(&previous), incoming, None).unwrap();
    assert_eq!(merged.used_tokens, 25000);
    assert_eq!(merged.window_tokens, 1000000);
    assert_eq!(merged.cost_usd, Some(1.5));
    assert_eq!(merged.source, "claude-jsonl-compaction");
}

#[test]
fn claude_context_feed_is_off_by_default_and_rejects_foreign_session() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let config: crate::config::ConfigState = std::sync::Arc::new(parking_lot::Mutex::new(
        crate::config::schema::AppConfig::default(),
    ));
    assert!(fixture._app.manage(config.clone()));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opaque".into());
        tab.agent_session_id = Some("own".into());
        tab.session_binding_nonce = Some("nonce".into());
    }
    assert!(!apply(
        &fixture.service,
        &tab_id,
        "own",
        "nonce",
        usage(4, 0)
    ));
    config
        .lock()
        .agents
        .entry("opaque".into())
        .or_default()
        .extra
        .insert("context_feed".into(), toml::Value::Boolean(true));
    assert!(!apply(
        &fixture.service,
        &tab_id,
        "foreign",
        "nonce",
        usage(4, 200000)
    ));
    assert!(apply(
        &fixture.service,
        &tab_id,
        "own",
        "nonce",
        usage(4, 200000)
    ));
    let wire = serde_json::to_value(fixture.service.mux.read().snapshot()).unwrap();
    assert_eq!(wire["tabs"][&tab_id]["context"]["used_tokens"], 4);
    assert!(!update(&fixture.service, &tab_id, "foreign", "nonce", None));
    assert!(update(&fixture.service, &tab_id, "own", "nonce", None));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .context
        .is_none());
}
