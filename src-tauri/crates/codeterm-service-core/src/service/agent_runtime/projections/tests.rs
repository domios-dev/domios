use super::*;

fn window(name: &str, used: f64, minutes: u64) -> RuntimeRateLimit {
    RuntimeRateLimit {
        window: name.into(),
        used_percent: used,
        resets_at: Some(9000),
        window_minutes: Some(minutes),
    }
}

#[test]
fn all_windows_contribute_to_dispatch_headroom_at_host_receipt_time() {
    let windows = vec![
        window("short", 8.0, 300),
        window("long", 20.0, 10080),
        window("other", 97.0, 60),
    ];
    let sample = rate_sample("fixture", &windows, 1234);
    let dir = tempfile::tempdir().unwrap();
    let store = crate::usage::UsageStore::open(dir.path()).unwrap();
    store.insert_runtime_snapshot(&sample).unwrap();
    let row = store.get_latest("fixture", None).unwrap().unwrap();
    assert_eq!(row.captured_at, 1234);
    assert_eq!(crate::usage::allowance_remaining_pct(&row), 3.0);
    assert_eq!(row.session_resets_at, Some(9000));
    assert!(row.spend_cents.is_none());
}

#[test]
fn empty_rates_report_no_limits_or_tokens() {
    let sample = rate_sample("fixture", &[], 10);
    assert!(!sample.limits_reported);
    assert_eq!(sample.session_pct, None);
    assert_eq!(sample.weekly_pct, None);
}
#[test]
fn ordinary_two_windows_keep_the_existing_session_and_weekly_meaning() {
    let sample = rate_sample(
        "fixture",
        &[window("short", 10.0, 300), window("long", 80.0, 10080)],
        100,
    );
    assert_eq!(sample.session_pct, Some(10.0));
    assert_eq!(sample.weekly_pct, Some(80.0));
}

#[test]
fn token_only_reading_is_visible_without_fabricating_allowance() {
    let reading = UsageProjection {
        tab_id: "own".into(),
        thread_id: Some("thread".into()),
        provider: "fixture".into(),
        nonce: "nonce".into(),
        epoch: 1,
        billing_mode: codeterm_usage::contract::BillingMode::Api,
        captured_at: 10,
        token_usage: Some(Reading {
            captured_at: 10,
            value: TokenUsage {
                cost_micros: None,
                input_tokens: 12,
                output_tokens: 3,
                cache_read_tokens: Some(4),
                cache_write_tokens: None,
                used_tokens: Some(15),
                window_tokens: Some(100),
            },
        }),
        rate_limits: None,
        resets: None,
        account_credits: None,
    };
    let wire = enrich_latest(None, "fixture", vec![reading]).unwrap();
    assert_eq!(
        wire["runtime_usage"][0]["token_usage"]["value"]["input_tokens"],
        12
    );
    assert_eq!(wire["runtime_usage"][0]["token_usage"]["captured_at"], 10);
    assert!(wire.get("session_pct").is_none());
    assert_eq!(wire["limits_reported"], false);
    assert!(wire["runtime_usage"][0].get("nonce").is_none());
    assert!(enrich_latest(None, "fixture", vec![]).unwrap().is_null());
}
#[test]
fn runtime_usage_binding_changes_clear_tokens_but_keep_account_limits() {
    let mut readings = HashMap::new();
    let mut context: RuntimeContext = serde_json::from_value(serde_json::json!({
        "epoch":1,"tabId":"own","cwd":"/fixture","hostVersion":"1",
        "bindingNonce":"nonce","settings":{},"providerSettings":{},
        "activeTurns":[],"threadParents":[],"claimedSends":[]
    }))
    .unwrap();
    let first = projection(&mut readings, "fixture", &context);
    first.billing_mode = codeterm_usage::contract::BillingMode::Subscription;
    first.rate_limits = Some(Reading {
        captured_at: 10,
        value: vec![window("primary", 20.0, 300)],
    });
    first.resets = Some(codeterm_plugin::agent_runtime::ResetCredits {
        available_count: 3,
        next_expiry: None,
        credits: None,
    });
    first.account_credits = Some(RuntimeAccountCredits::Balance {
        balance: "12.50".into(),
    });
    context.thread_id = Some("thread".into());
    let loaded = projection(&mut readings, "fixture", &context);
    assert_eq!(
        loaded.billing_mode,
        codeterm_usage::contract::BillingMode::Subscription
    );
    assert_eq!(
        loaded.rate_limits.as_ref().unwrap().value[0].used_percent,
        20.0
    );
    assert_eq!(loaded.resets.as_ref().unwrap().available_count, 3);
    assert_eq!(
        loaded.account_credits,
        Some(RuntimeAccountCredits::Balance {
            balance: "12.50".into()
        })
    );
    context.binding_nonce = "new".into();
    let rebound = projection(&mut readings, "fixture", &context);
    assert_eq!(
        rebound.billing_mode,
        codeterm_usage::contract::BillingMode::Unknown
    );
    assert!(rebound.resets.is_none());
    assert!(rebound.account_credits.is_none());
    assert!(rebound.rate_limits.is_none());
    assert!(rebound.token_usage.is_none());
}

#[test]
fn partial_confirmation_preserves_other_runtime_controls() {
    let prior = RuntimeSettings {
        model: Some("old".into()),
        effort: Some("high".into()),
        mode: Some("mode".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_settings(
            &prior,
            RuntimeSettings {
                model: Some("new".into()),
                ..Default::default()
            }
        ),
        RuntimeSettings {
            model: Some("new".into()),
            ..prior
        }
    );
}

fn mode_option(current: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "opaque-mode",
        "name": "Mode",
        "category": "mode",
        "currentValue": current,
        "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}],
    })
}

fn reported(value: serde_json::Value) -> RuntimeSettings {
    serde_json::from_value(value).unwrap()
}

fn both_modes(settings: &RuntimeSettings) -> (Option<&str>, Option<&str>) {
    (settings.mode.as_deref(), settings.effective_mode())
}

#[test]
fn a_mode_option_change_moves_the_session_mode_with_it() {
    let prior = reported(serde_json::json!({"mode": "a", "configOptions": [mode_option("a")]}));
    let merged = merge_settings(
        &prior,
        reported(serde_json::json!({"configOptions": [mode_option("b")]})),
    );
    assert_eq!(both_modes(&merged), (Some("b"), Some("b")));
}

#[test]
fn a_session_mode_report_moves_the_mode_option_with_it() {
    let prior = reported(serde_json::json!({"mode": "a", "configOptions": [mode_option("a")]}));
    let merged = merge_settings(&prior, reported(serde_json::json!({"mode": "b"})));
    assert_eq!(both_modes(&merged), (Some("b"), Some("b")));
}

#[test]
fn a_report_carrying_both_settles_on_the_option() {
    let merged = merge_settings(
        &RuntimeSettings::default(),
        reported(serde_json::json!({"mode": "a", "configOptions": [mode_option("b")]})),
    );
    assert_eq!(both_modes(&merged), (Some("b"), Some("b")));
}

#[test]
fn a_session_mode_the_option_cannot_express_stands_and_the_option_reads_it() {
    let merged = merge_settings(
        &RuntimeSettings::default(),
        reported(serde_json::json!({"mode": "c", "configOptions": [mode_option("a")]})),
    );
    assert_eq!(both_modes(&merged), (Some("c"), Some("c")));
}

#[test]
fn a_session_without_a_mode_option_keeps_its_session_mode() {
    let merged = merge_settings(
        &RuntimeSettings::default(),
        reported(serde_json::json!({"mode": "a", "configOptions": []})),
    );
    assert_eq!(both_modes(&merged), (Some("a"), Some("a")));
}

fn declare_usage(svc: &ServiceContext, provider: &str, enabled: bool) {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let manifest=PluginManifest::from_json_str(&serde_json::json!({
        "id":provider,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{"runtime":"stdio-jsonrpc","usage":enabled,
            "command":{"binary":"fixture-cli","args":[]},"requestMethods":{"initialize":["initialize"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    svc.agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            std::sync::Arc::new(
                PluginVm::from_source(provider, "module.exports={}".into()).unwrap(),
            ),
        )));
}

#[test]
fn context_reports_and_token_usage_project_only_observed_fill_and_cost() {
    use super::super::{engine, subscriber};
    use codeterm_plugin::agent_runtime::AgentRuntimeEvent;
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    declare_usage(&fixture.service, &session.provider, true);
    let epoch = session.context.lock().epoch;
    let report = |used, window, cost| AgentRuntimeEvent::Context {
        thread_id: "own-thread".into(),
        used_tokens: used,
        window_tokens: window,
        cost_micros: cost,
    };
    crate::async_runtime::block_on(subscriber::publish_events(
        &fixture.service,
        &session,
        epoch,
        vec![report(25, 100, Some(1_250_000))],
        10,
    ))
    .unwrap();
    let wire = serde_json::to_value(fixture.service.mux.read().snapshot()).unwrap();
    assert_eq!(
        wire["tabs"][&tab]["context"],
        serde_json::json!({
            "used_tokens":25,"window_tokens":100,"cost_usd":1.25,"source":"agent-runtime"
        })
    );
    assert!(
        USAGE
            .lock()
            .get(&tab)
            .and_then(|reading| reading.token_usage.as_ref())
            .is_none(),
        "A context report must not create token counters"
    );
    crate::async_runtime::block_on(subscriber::publish_events(
        &fixture.service,
        &session,
        epoch,
        vec![report(4, 200, None)],
        11,
    ))
    .unwrap();
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .context
        .clone()
        .unwrap();
    assert_eq!(current.used_tokens, 4);
    assert_eq!(current.window_tokens, 200);
    assert_eq!(current.cost_usd, None);
    let mut token_usage = TokenUsage {
        input_tokens: 1000,
        output_tokens: 2000,
        cost_micros: Some(500_000),
        cache_read_tokens: None,
        cache_write_tokens: None,
        used_tokens: Some(6),
        window_tokens: Some(300),
    };
    usage(&fixture.service, &session, token_usage.clone(), 12);
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .context
        .clone()
        .unwrap();
    assert_eq!(current.used_tokens, 6);
    assert_eq!(current.window_tokens, 300);
    assert_eq!(current.cost_usd, Some(0.5));
    token_usage.used_tokens = None;
    token_usage.window_tokens = None;
    usage(&fixture.service, &session, token_usage.clone(), 13);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .context
            .as_ref(),
        Some(&current)
    );
    token_usage.used_tokens = Some(999);
    usage(&fixture.service, &session, token_usage, 14);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .context
            .as_ref(),
        Some(&current)
    );
    let _ = crate::async_runtime::block_on(subscriber::publish_events(
        &fixture.service,
        &session,
        epoch,
        vec![AgentRuntimeEvent::Context {
            thread_id: "foreign-thread".into(),
            used_tokens: 99,
            window_tokens: 100,
            cost_micros: None,
        }],
        15,
    ));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .context
            .as_ref(),
        Some(&current)
    );
    crate::async_runtime::block_on(subscriber::publish_events(
        &fixture.service,
        &session,
        epoch,
        vec![report(0, 300, Some(0))],
        16,
    ))
    .unwrap();
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .context
        .clone()
        .unwrap();
    assert_eq!(current.used_tokens, 0);
    assert_eq!(current.cost_usd, Some(0.0));
}

#[test]
fn runtime_context_requires_declared_usage_and_exact_current_binding() {
    use super::super::engine;
    use crate::multiplexer::types::{Tab, TabBacking};
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    declare_usage(&fixture.service, &session.provider, false);
    context(&fixture.service, &session, 5, 100, None);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .context
        .is_none());
    declare_usage(&fixture.service, &session.provider, true);
    context(&fixture.service, &session, 5, 100, None);
    let original = fixture.service.mux.read().get_tab(&tab).unwrap().clone();
    assert_eq!(original.context.as_ref().unwrap().used_tokens, 5);
    for changed in [
        Tab {
            agent_session_id: Some("foreign-session".into()),
            ..original.clone()
        },
        Tab {
            session_binding_nonce: Some("foreign-nonce".into()),
            ..original.clone()
        },
        Tab {
            reported_session_binding_nonce: Some("foreign-nonce".into()),
            ..original.clone()
        },
        Tab {
            cwd: Some("foreign-cwd".into()),
            ..original.clone()
        },
        Tab {
            agent_type: Some("foreign-provider".into()),
            ..original.clone()
        },
        Tab {
            backing: TabBacking::ChatBackend {
                plugin_id: session.provider.clone(),
                session_id: "foreign-session".into(),
            },
            ..original.clone()
        },
    ] {
        *fixture.service.mux.write().get_tab_mut(&tab).unwrap() = changed;
        context(&fixture.service, &session, 50, 100, Some(1_000_000));
        assert_eq!(
            fixture.service.mux.read().get_tab(&tab).unwrap().context,
            original.context
        );
    }
    *fixture.service.mux.write().get_tab_mut(&tab).unwrap() = original.clone();
    context(&fixture.service, &session, 10_000_001, 100, None);
    context(&fixture.service, &session, 5, 10_000_001, None);
    context(&fixture.service, &session, 5, 100, Some(u64::MAX));
    assert_eq!(
        fixture.service.mux.read().get_tab(&tab).unwrap().context,
        original.context
    );
}
