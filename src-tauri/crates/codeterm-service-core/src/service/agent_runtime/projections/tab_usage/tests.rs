use super::super::{rate_sample, Reading, UsageProjection};
use super::*;
use crate::service::agent_runtime::engine::test_support::runtime_fixture;
use codeterm_plugin::agent_runtime::RuntimeRateLimit;
use codeterm_types::agent::context::TurnContext;

/// The same cases `tabUsageSource` is pinned to in usageTracking.test.ts.
#[test]
fn the_source_follows_the_declared_kind_exactly_as_the_ui_does() {
    use super::RuntimeProfileLimitsSource as Declared;
    let meter = |provider: &str| TabLimitsSource::Meter {
        provider: provider.into(),
    };
    let cases = [
        (
            ("fixture-acp", true, true),
            Some((
                "borrowing",
                Declared::Account {
                    provider: "fixture-native".into(),
                },
            )),
            TabLimitsSource::Account {
                provider: "fixture-native".into(),
            },
        ),
        (
            ("fixture-acp", true, false),
            Some((
                "balanced",
                Declared::Balance {
                    reader: "fixture-balance".into(),
                    credential_env: "KEY".into(),
                },
            )),
            TabLimitsSource::Balance {
                provider: "fixture-acp".into(),
                account_id: "profile:balanced".into(),
            },
        ),
        (
            ("fixture-acp", true, false),
            Some((
                "quoted",
                Declared::Quota {
                    reader: "fixture-quota".into(),
                },
            )),
            TabLimitsSource::Quota {
                provider: "fixture-acp".into(),
                account_id: "profile:quoted".into(),
            },
        ),
        (
            ("fixture-acp", true, true),
            Some((
                "opaque",
                Declared::Unavailable {
                    reason: "Opaque reason".into(),
                },
            )),
            TabLimitsSource::Unavailable {
                provider: "fixture-acp".into(),
                reason: "Opaque reason".into(),
            },
        ),
        (("fixture-acp", true, true), None, meter("fixture-acp")),
        (
            ("fixture-pty", false, false),
            Some((
                "ignored",
                Declared::Unavailable {
                    reason: "Opaque reason".into(),
                },
            )),
            meter("fixture-pty"),
        ),
    ];
    for ((provider, chat_backend, runtime_usage), declared, expected) in cases {
        assert_eq!(
            TabLimitsSource::resolve(provider, chat_backend, runtime_usage, declared),
            expected
        );
    }
    assert!(matches!(
        TabLimitsSource::resolve("fixture-acp", true, false, None),
        TabLimitsSource::Unavailable { .. }
    ));
}

fn subscription_reading(tab: &str, thread: &str, limits_at: u64) -> Value {
    json!({
        "tab_id":tab,"thread_id":thread,"billing_mode":"subscription","captured_at":limits_at,
        "rate_limits":{"captured_at":limits_at,"value":[
            {"window":"secondary","usedPercent":80.0,"resetsAt":200},
            {"window":"primary","usedPercent":20.0,"resetsAt":100},
        ]},
        "resets":{"availableCount":3},"account_credits":{"kind":"balance","balance":"12.50"},
    })
}

fn stored_row(captured_at: i64, session_pct: f64) -> Value {
    json!({
        "id":7,"provider":"fixture-native","account_id":null,"captured_at":captured_at,"parsed_ok":true,
        "billing_mode":"subscription","limits_reported":true,"session_pct":session_pct,"weekly_pct":1.0,
    })
}

#[test]
fn a_tab_with_its_own_reading_resolves_like_latest_usage_snapshot() {
    let own = |row: Value, reading: Value| {
        let mut snapshot = row;
        snapshot["runtime_usage"] = json!([reading]);
        own_view(&snapshot, "own", Some("thread"))
    };

    let newer_row = own(
        stored_row(50, 9.0),
        subscription_reading("own", "thread", 10),
    );
    assert_eq!(newer_row["session_pct"], 9.0);
    assert_eq!(newer_row["resets"]["availableCount"], 3);
    assert_eq!(newer_row["account_credits"]["balance"], "12.50");

    let newer_reading = own(
        stored_row(5, 9.0),
        subscription_reading("own", "thread", 10),
    );
    assert_eq!(newer_reading["session_pct"], 20.0);
    assert_eq!(newer_reading["session_resets_at"], 100);
    assert_eq!(newer_reading["weekly_pct"], 80.0);
    assert_eq!(newer_reading["captured_at"], 10);
    assert_eq!(newer_reading["limits_reported"], true);
    assert_eq!(newer_reading["id"], 0);

    let mut api = subscription_reading("own", "thread", 10);
    api["billing_mode"] = json!("api");
    let api_session = own(stored_row(50, 9.0), api);
    assert_eq!(api_session["limits_reported"], false);
    assert!(api_session["session_pct"].is_null());
    assert_eq!(api_session["billing_mode"], "api");

    let sibling_only = own(
        stored_row(50, 9.0),
        subscription_reading("sibling", "thread", 10),
    );
    assert!(
        sibling_only.is_null(),
        "a sibling's reading never speaks for this tab"
    );

    let row_only = own_view(&stored_row(50, 9.0), "own", Some("thread"));
    assert_eq!(row_only["session_pct"], 9.0);
    assert!(own_view(&Value::Null, "own", None).is_null());
}

#[test]
fn a_borrowing_tab_resolves_like_account_usage_snapshot() {
    let with = |row: Value, readings: Vec<Value>| {
        let mut snapshot = row;
        snapshot["runtime_usage"] = json!(readings);
        account_view(&snapshot)
    };

    let mut stale = subscription_reading("older", "a", 10);
    stale["rate_limits"]["value"][1]["usedPercent"] = json!(55.0);
    let mut token_only = subscription_reading("tokens", "b", 99);
    token_only["rate_limits"] = Value::Null;
    let freshest = with(
        stored_row(5, 9.0),
        vec![stale, token_only, subscription_reading("newer", "c", 20)],
    );
    assert_eq!(freshest["session_pct"], 20.0);
    assert_eq!(freshest["resets"]["availableCount"], 3);

    let mut api = subscription_reading("api", "d", 10);
    api["billing_mode"] = json!("api");
    let only_the_row = with(stored_row(5, 9.0), vec![api.clone()]);
    assert_eq!(only_the_row["session_pct"], 9.0);
    let mut unparsed = stored_row(5, 9.0);
    unparsed["parsed_ok"] = json!(false);
    assert!(with(unparsed, vec![api]).is_null());

    assert_eq!(account_view(&stored_row(5, 9.0))["session_pct"], 9.0);
    assert!(account_view(&Value::Null).is_null());
}

#[test]
fn a_plugin_read_is_due_when_missing_failed_or_past_its_interval() {
    assert!(plugin_read_due(None, 1000, 100));
    assert!(plugin_read_due(Some((false, 1000)), 1000, 100));
    assert!(plugin_read_due(Some((true, 899)), 1000, 100));
    assert!(!plugin_read_due(Some((true, 900)), 1000, 100));
}

const OPAQUE_PROFILE: &str = "opaque";
const BORROWING_PROFILE: &str = "borrowing";
const UNDECLARED_PROFILE: &str = "undeclared";
const NATIVE: &str = "fixture-native";

fn own_provider(svc: &ServiceContext, tab: &str) -> String {
    svc.mux
        .read()
        .get_tab(tab)
        .unwrap()
        .agent_type
        .clone()
        .unwrap()
}

/// Declares the fixture's launch profiles and puts the tab on one of them.
fn put_on_profile(svc: &ServiceContext, tab: &str, profile: &str) {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let provider = own_provider(svc, tab);
    let manifest = PluginManifest::from_json_str(
        &json!({
            "id":provider,"version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{"runtime":"stdio-jsonrpc","usage":true,
                "command":{"binary":"fixture-cli","args":[]},"requestMethods":{"initialize":["initialize"]},"notificationMethods":[],
                "launchProfiles":{
                    (BORROWING_PROFILE):{"binary":"fixture-cli","args":[],"limitsSource":{"kind":"account","provider":NATIVE}},
                    (OPAQUE_PROFILE):{"binary":"fixture-cli","args":[],"limitsSource":{"kind":"unavailable","reason":"Opaque reason"}},
                    (UNDECLARED_PROFILE):{"binary":"fixture-cli","args":[]},
                }}},
            "permissions":{"subprocess":{"allow":["fixture-cli"]}}
        })
        .to_string(),
    )
    .unwrap();
    svc.agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            std::sync::Arc::new(
                PluginVm::from_source(&provider, "module.exports={}".into()).unwrap(),
            ),
        )));
    svc.mux
        .write()
        .get_tab_mut(tab)
        .unwrap()
        .agent_runtime_config = Some(json!({"launchProfile":profile}));
}

fn window(name: &str, used: f64, minutes: u64) -> RuntimeRateLimit {
    RuntimeRateLimit {
        window: name.into(),
        used_percent: used,
        resets_at: Some(9000),
        window_minutes: Some(minutes),
    }
}

fn store_limits(svc: &ServiceContext, provider: &str) {
    let sample = rate_sample(
        provider,
        &[window("short", 10.0, 300), window("long", 80.0, 10080)],
        1000,
    );
    svc.usage_store.insert_runtime_snapshot(&sample).unwrap();
}

#[test]
fn a_tab_on_an_account_profile_reports_the_native_accounts_limits() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture();
    let svc = &fixture.service;
    put_on_profile(svc, &tab, BORROWING_PROFILE);
    store_limits(svc, NATIVE);

    let wire = for_tab(svc, &tab).unwrap();

    assert_eq!(wire["limits_reported"], true);
    assert_eq!(wire["provider"], NATIVE);
    assert_eq!(wire["session_pct"], 10.0);
    assert_eq!(wire["weekly_pct"], 80.0);
    assert_eq!(
        wire["limits_source"],
        json!({"kind":"account","provider":NATIVE})
    );
    assert!(wire.get("unavailable_reason").is_none());
}

#[test]
fn a_tab_on_an_unread_account_profile_says_nothing_has_been_read() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture();
    let svc = &fixture.service;
    put_on_profile(svc, &tab, BORROWING_PROFILE);

    let wire = for_tab(svc, &tab).unwrap();

    assert_eq!(wire["limits_reported"], false);
    assert!(wire["unavailable_reason"]
        .as_str()
        .unwrap()
        .contains(NATIVE));
}

#[test]
fn a_profile_declaring_unavailable_keeps_its_reason_and_reports_the_context_fill() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture();
    let svc = &fixture.service;
    put_on_profile(svc, &tab, OPAQUE_PROFILE);
    store_limits(svc, &own_provider(svc, &tab));
    svc.mux.write().get_tab_mut(&tab).unwrap().context = Some(TurnContext {
        used_tokens: 50,
        window_tokens: 200,
        cost_usd: None,
        source: "agent-runtime".into(),
    });

    let wire = for_tab(svc, &tab).unwrap();

    assert_eq!(wire["limits_reported"], false);
    assert_eq!(wire["unavailable_reason"], "Opaque reason");
    assert_eq!(wire["limits_source"]["kind"], "unavailable");
    assert!(
        wire["session_pct"].is_null(),
        "a provider row never speaks for a source with nothing to read"
    );
    assert_eq!(wire["context"]["used_tokens"], 50);
    assert_eq!(wire["context"]["window_tokens"], 200);
}

#[test]
fn the_own_reading_stays_binding_checked_while_the_limits_answer_does_not_depend_on_it() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture();
    let svc = &fixture.service;
    put_on_profile(svc, &tab, UNDECLARED_PROFILE);
    let provider = own_provider(svc, &tab);
    let nonce = svc
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .session_binding_nonce
        .clone()
        .unwrap();
    USAGE.lock().insert(
        tab.clone(),
        UsageProjection {
            tab_id: tab.clone(),
            thread_id: None,
            provider,
            nonce,
            epoch: 1,
            billing_mode: codeterm_usage::contract::BillingMode::Subscription,
            captured_at: 10,
            token_usage: None,
            rate_limits: Some(Reading {
                captured_at: 10,
                value: vec![window("primary", 20.0, 300)],
            }),
            resets: None,
            account_credits: None,
        },
    );

    let bound = for_tab(svc, &tab).unwrap();
    assert_eq!(bound["runtime_usage"][0]["tab_id"], tab.as_str());
    assert_eq!(bound["runtime_usage"][0]["thread_id"], "own-thread");
    assert_eq!(bound["session_pct"], 20.0);
    assert_eq!(bound["limits_reported"], true);

    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .reported_session_binding_nonce = Some("foreign-nonce".into());
    let unbound = for_tab(svc, &tab).unwrap();
    assert!(unbound.get("runtime_usage").is_none());
    assert_eq!(unbound["limits_reported"], false);
    assert!(unbound["unavailable_reason"]
        .as_str()
        .unwrap()
        .contains(UNDECLARED_PROFILE));
}

#[test]
fn disabled_usage_tracking_reports_nothing_and_says_so() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture();
    let svc = &fixture.service;
    put_on_profile(svc, &tab, BORROWING_PROFILE);
    store_limits(svc, NATIVE);
    svc.ext::<crate::config::ConfigState>()
        .lock()
        .agents
        .entry(NATIVE.to_string())
        .or_default()
        .usage_tracking_enabled = Some(false);

    let wire = for_tab(svc, &tab).unwrap();

    assert_eq!(wire["limits_reported"], false);
    assert!(wire["session_pct"].is_null());
    assert!(wire["unavailable_reason"]
        .as_str()
        .unwrap()
        .contains("disabled"));
}
