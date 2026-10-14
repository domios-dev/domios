use super::{engine::Session, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    RuntimeContext, RuntimeRateLimit, RuntimeSettings, TokenUsage,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Clone, Serialize)]
struct Reading<T> {
    captured_at: u64,
    value: T,
}

#[derive(Clone, Serialize)]
struct UsageProjection {
    tab_id: String,
    thread_id: Option<String>,
    #[serde(skip)]
    provider: String,
    #[serde(skip)]
    nonce: String,
    #[serde(skip)]
    epoch: u64,
    billing_mode: codeterm_types::usage::BillingMode,
    captured_at: u64,
    token_usage: Option<Reading<TokenUsage>>,
    rate_limits: Option<Reading<Vec<RuntimeRateLimit>>>,
}

static USAGE: LazyLock<Mutex<HashMap<String, UsageProjection>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn controls(svc: &ServiceContext, session: &Session, settings: RuntimeSettings) {
    let context = session.context.lock().clone();
    svc.record_runtime_controls(&session.provider, &context, &settings);
    *session.controls_confirmation.lock() = Some(settings.clone());
    let settings = merge_settings(&context.settings, settings);
    session.context.lock().settings = settings;
}

fn merge_settings(previous: &RuntimeSettings, confirmed: RuntimeSettings) -> RuntimeSettings {
    RuntimeSettings {
        model: confirmed.model.or_else(|| previous.model.clone()),
        effort: confirmed.effort.or_else(|| previous.effort.clone()),
        mode: confirmed.mode.or_else(|| previous.mode.clone()),
        ..previous.clone()
    }
}

fn projection<'a>(
    readings: &'a mut HashMap<String, UsageProjection>,
    provider: &str,
    context: &RuntimeContext,
) -> &'a mut UsageProjection {
    let reading = readings
        .entry(context.tab_id.clone())
        .or_insert_with(|| UsageProjection {
            tab_id: context.tab_id.clone(),
            thread_id: context.thread_id.clone(),
            provider: provider.into(),
            nonce: context.binding_nonce.clone(),
            epoch: context.epoch,
            billing_mode: Default::default(),
            captured_at: 0,
            token_usage: None,
            rate_limits: None,
        });
    if reading.provider != provider || reading.nonce != context.binding_nonce {
        reading.provider = provider.into();
        reading.nonce = context.binding_nonce.clone();
        reading.thread_id = context.thread_id.clone();
        reading.billing_mode = Default::default();
        reading.token_usage = None;
        reading.rate_limits = None;
    }
    if reading.thread_id != context.thread_id {
        reading.thread_id = context.thread_id.clone();
        reading.token_usage = None;
    }
    reading.epoch = context.epoch;
    reading
}

fn publishes_usage(svc: &ServiceContext, session: &Session) -> bool {
    svc.agent_registry
        .with_provider(&session.provider, |provider| {
            provider
                .agent_runtime()
                .is_some_and(|capability| capability.usage)
        })
        .unwrap_or(false)
}

pub fn billing(
    svc: &ServiceContext,
    session: &Session,
    mode: codeterm_types::usage::BillingMode,
    at: u64,
) {
    if !publishes_usage(svc, session) {
        return;
    }
    let context = session.context.lock().clone();
    let mut readings = USAGE.lock();
    let reading = projection(&mut readings, &session.provider, &context);
    reading.billing_mode = mode;
    reading.captured_at = at;
    if mode != codeterm_types::usage::BillingMode::Subscription {
        reading.rate_limits = None;
    }
    if mode == codeterm_types::usage::BillingMode::Unknown {
        reading.token_usage = None;
    }
    drop(readings);
    svc.notify_event("usage-snapshot-updated", serde_json::json!({"provider":session.provider,"account_id":null,"tab_id":context.tab_id,"captured_at":at}));
}

pub fn usage(svc: &ServiceContext, session: &Session, usage: TokenUsage, at: u64) {
    if !publishes_usage(svc, session) {
        return;
    }
    let context = session.context.lock().clone();
    let mut readings = USAGE.lock();
    let reading = projection(&mut readings, &session.provider, &context);
    reading.captured_at = at;
    reading.token_usage = Some(Reading {
        captured_at: at,
        value: usage,
    });
    drop(readings);
    svc.notify_event(
        "usage-snapshot-updated",
        serde_json::json!({"provider":session.provider,"account_id":null}),
    );
}

pub async fn rate_limits(
    svc: &ServiceContext,
    session: &Session,
    limits: Vec<RuntimeRateLimit>,
    at: u64,
) -> Result<(), RuntimeFailure> {
    if !publishes_usage(svc, session) {
        return Ok(());
    }
    let context = session.context.lock().clone();
    let sample = rate_sample(&session.provider, &limits, at);
    let store = svc.usage_store.clone();
    let snapshot_id = tokio::task::spawn_blocking(move || store.insert_runtime_snapshot(&sample))
        .await
        .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
        .map_err(RuntimeFailure::HookFailed)?;
    let mut readings = USAGE.lock();
    let reading = projection(&mut readings, &session.provider, &context);
    reading.captured_at = at;
    reading.rate_limits = Some(Reading {
        captured_at: at,
        value: limits,
    });
    drop(readings);
    svc.notify_event(
        "usage-snapshot-updated",
        serde_json::json!({"provider":session.provider,"account_id":null,"snapshot_id":snapshot_id}),
    );
    Ok(())
}

fn rate_sample(
    provider: &str,
    limits: &[RuntimeRateLimit],
    at: u64,
) -> codeterm_usage::UsageSample {
    let highest = limits
        .iter()
        .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent));
    let longest = limits.iter().max_by_key(|window| window.window_minutes);
    let shortest = limits.iter().min_by_key(|window| window.window_minutes);
    let session = if highest == longest {
        shortest
    } else {
        highest
    };
    codeterm_usage::UsageSample {
        provider: provider.into(),
        billing_mode: codeterm_types::usage::BillingMode::Subscription,
        limits_reported: !limits.is_empty(),
        account_id: None,
        captured_at_ms: at as i64,
        session_pct: session.map(|window| window.used_percent as f32),
        session_resets_at_ms: session
            .and_then(|window| window.resets_at)
            .map(|at| at as i64),
        weekly_pct: longest
            .filter(|window| Some(*window) != session)
            .map(|window| window.used_percent as f32),
        weekly_resets_at_ms: longest
            .filter(|window| Some(*window) != session)
            .and_then(|window| window.resets_at)
            .map(|at| at as i64),
        spend_cents: None,
        spend_limit_cents: None,
        spend_remaining_cents: None,
        spend_resets_at_ms: None,
        raw_text: String::new(),
    }
}

pub fn latest(
    svc: &ServiceContext,
    provider: &str,
    account: Option<&str>,
) -> Result<serde_json::Value, String> {
    let row = svc
        .usage_store
        .get_latest_with_stale_reason(provider, account)?;
    let result = serde_json::to_value(row).map_err(|error| error.to_string())?;
    if account.is_some() {
        return Ok(result);
    }
    let mux = svc.mux.read();
    let mut readings = USAGE.lock();
    readings.retain(|tab, _| mux.get_tab(tab).is_some());
    let mut current = readings
        .values()
        .filter(|reading| {
            reading.provider == provider
                && mux.get_tab(&reading.tab_id).is_some_and(|tab| {
                    tab.agent_type.as_deref() == Some(provider)
                        && (tab.agent_session_id == reading.thread_id
                            || reading.thread_id.is_none())
                        && tab.session_binding_nonce.as_deref() == Some(&reading.nonce)
                        && tab.reported_session_binding_nonce.as_deref() == Some(&reading.nonce)
                })
        })
        .cloned()
        .map(|mut reading| {
            if reading.thread_id.is_none() {
                reading.thread_id = mux
                    .get_tab(&reading.tab_id)
                    .and_then(|tab| tab.agent_session_id.clone());
            }
            reading
        })
        .collect::<Vec<_>>();
    current.sort_by(|a, b| a.tab_id.cmp(&b.tab_id));
    enrich_latest(result, provider, current)
}

fn enrich_latest(
    mut result: serde_json::Value,
    provider: &str,
    current: Vec<UsageProjection>,
) -> Result<serde_json::Value, String> {
    if !current.is_empty() {
        if result.is_null() {
            result = serde_json::json!({"provider": provider, "limits_reported": false});
        }
        result["runtime_usage"] =
            serde_json::to_value(current).map_err(|error| error.to_string())?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
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
            billing_mode: codeterm_types::usage::BillingMode::Api,
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
        };
        let wire = enrich_latest(serde_json::Value::Null, "fixture", vec![reading]).unwrap();
        assert_eq!(
            wire["runtime_usage"][0]["token_usage"]["value"]["input_tokens"],
            12
        );
        assert_eq!(wire["runtime_usage"][0]["token_usage"]["captured_at"], 10);
        assert!(wire.get("session_pct").is_none());
        assert_eq!(wire["limits_reported"], false);
        assert!(wire["runtime_usage"][0].get("nonce").is_none());
        assert!(enrich_latest(serde_json::Value::Null, "fixture", vec![])
            .unwrap()
            .is_null());
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
        first.billing_mode = codeterm_types::usage::BillingMode::Subscription;
        first.rate_limits = Some(Reading {
            captured_at: 10,
            value: vec![window("primary", 20.0, 300)],
        });
        context.thread_id = Some("thread".into());
        let loaded = projection(&mut readings, "fixture", &context);
        assert_eq!(
            loaded.billing_mode,
            codeterm_types::usage::BillingMode::Subscription
        );
        assert_eq!(
            loaded.rate_limits.as_ref().unwrap().value[0].used_percent,
            20.0
        );
        context.binding_nonce = "new".into();
        let rebound = projection(&mut readings, "fixture", &context);
        assert_eq!(
            rebound.billing_mode,
            codeterm_types::usage::BillingMode::Unknown
        );
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
}
