use super::{engine::Session, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    RuntimeAccountCredits, RuntimeContext, RuntimeRateLimit, RuntimeSettings, TokenUsage,
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
    billing_mode: codeterm_usage::contract::BillingMode,
    captured_at: u64,
    token_usage: Option<Reading<TokenUsage>>,
    rate_limits: Option<Reading<Vec<RuntimeRateLimit>>>,
    resets: Option<codeterm_plugin::agent_runtime::ResetCredits>,
    account_credits: Option<RuntimeAccountCredits>,
}

static USAGE: LazyLock<Mutex<HashMap<String, UsageProjection>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn controls(svc: &ServiceContext, session: &Session, settings: RuntimeSettings) {
    let context = session.context.lock().clone();
    if settings.model.is_some() && settings.model != context.settings.model {
        session.state.lock().notice = None;
    }
    svc.record_runtime_controls(&session.provider, &context, &settings);
    *session.controls_confirmation.lock() = Some(settings.clone());
    let settings = merge_settings(&context.settings, settings);
    super::full_access::observe_confirmed(session, &settings);
    session.context.lock().settings = settings;
    session.control_changes.send_replace(());
    super::engine::publish_controls(svc, &context.tab_id, session);
}

fn merge_settings(previous: &RuntimeSettings, confirmed: RuntimeSettings) -> RuntimeSettings {
    let session_mode = confirmed
        .config_options
        .is_none()
        .then(|| confirmed.mode.clone())
        .flatten();
    let mut merged = RuntimeSettings {
        commands: confirmed.commands.or_else(|| previous.commands.clone()),
        config_options: confirmed
            .config_options
            .or_else(|| previous.config_options.clone()),
        modes: confirmed.modes.or_else(|| previous.modes.clone()),
        service_tier: confirmed
            .service_tier
            .or_else(|| previous.service_tier.clone()),
        service_tier_availability: confirmed
            .service_tier_availability
            .or_else(|| previous.service_tier_availability.clone()),
        model: confirmed.model.or_else(|| previous.model.clone()),
        effort: confirmed.effort.or_else(|| previous.effort.clone()),
        mode: confirmed.mode.or_else(|| previous.mode.clone()),
        ..previous.clone()
    };
    match session_mode {
        Some(mode) => merged.follow_session_mode(mode),
        None => merged.settle_mode(),
    }
    merged
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
            resets: None,
            account_credits: None,
        });
    if reading.provider != provider || reading.nonce != context.binding_nonce {
        reading.provider = provider.into();
        reading.nonce = context.binding_nonce.clone();
        reading.thread_id = context.thread_id.clone();
        reading.billing_mode = Default::default();
        reading.token_usage = None;
        reading.rate_limits = None;
        reading.resets = None;
        reading.account_credits = None;
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
    mode: codeterm_usage::contract::BillingMode,
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
    if mode != codeterm_usage::contract::BillingMode::Subscription {
        reading.rate_limits = None;
        reading.resets = None;
        reading.account_credits = None;
    }
    if mode == codeterm_usage::contract::BillingMode::Unknown {
        reading.token_usage = None;
    }
    drop(readings);
    svc.notify_event("usage-snapshot-updated", serde_json::json!({"provider":session.provider,"account_id":null,"tab_id":context.tab_id,"captured_at":at}));
}

pub fn context(
    svc: &ServiceContext,
    session: &Session,
    used_tokens: u64,
    window_tokens: u64,
    cost_micros: Option<u64>,
) {
    if !publishes_usage(svc, session) || session.adapter.lock().is_none() {
        return;
    }
    let context = {
        let current = session.context.lock();
        RuntimeContext {
            tab_id: current.tab_id.clone(),
            binding_nonce: current.binding_nonce.clone(),
            thread_id: current.thread_id.clone(),
            cwd: current.cwd.clone(),
            epoch: current.epoch,
            ..Default::default()
        }
    };
    if session.state.lock().epoch != context.epoch {
        return;
    }
    crate::service::context_feed::apply_runtime(
        svc,
        &session.provider,
        &context,
        codeterm_types::agent::context::TurnContext {
            used_tokens,
            window_tokens,
            cost_usd: cost_micros.map(|cost| cost as f64 / 1_000_000.0),
            source: "agent-runtime".into(),
        },
    );
}

pub fn usage(svc: &ServiceContext, session: &Session, usage: TokenUsage, at: u64) {
    if let (Some(used), Some(window)) = (usage.used_tokens, usage.window_tokens) {
        context(svc, session, used, window, usage.cost_micros);
    }
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
    resets: Option<codeterm_plugin::agent_runtime::ResetCredits>,
    account_credits: Option<RuntimeAccountCredits>,
    at: u64,
) -> Result<(), RuntimeFailure> {
    if !publishes_usage(svc, session) {
        return Ok(());
    }
    let supported = super::reset_credits::supported(session).await;
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
    reading.resets = if supported { resets } else { None };
    reading.account_credits = account_credits;
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
        usage_details: None,
        provider: provider.into(),
        billing_mode: codeterm_usage::contract::BillingMode::Subscription,
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
    // A plugin-read balance has no earlier meter to fall back on, so its failure is the row.
    if account
        .and_then(codeterm_plugin::agent_runtime::limits_source::profile_from_balance_account_key)
        .is_some()
    {
        let row = svc.usage_store.get_latest_or_failure(provider, account)?;
        return serde_json::to_value(row).map_err(|error| error.to_string());
    }
    let row = svc
        .usage_store
        .get_latest_with_stale_reason(provider, account)?;
    if account.is_some() {
        return serde_json::to_value(row).map_err(|error| error.to_string());
    }
    // A profile's plugin-read balance is its own reading, never the provider's account meter.
    let row = row.filter(|row| {
        row.account_id
            .as_deref()
            .and_then(
                codeterm_plugin::agent_runtime::limits_source::profile_from_balance_account_key,
            )
            .is_none()
    });
    let result = serde_json::to_value(row).map_err(|error| error.to_string())?;
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

pub fn for_tab(svc: &ServiceContext, tab_id: &str) -> Result<serde_json::Value, String> {
    let (provider, thread_id, nonce) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
        let provider = tab.agent_type.clone().ok_or("Tab has no provider")?;
        let nonce = tab
            .session_binding_nonce
            .clone()
            .filter(|value| tab.reported_session_binding_nonce.as_ref() == Some(value))
            .ok_or("Tab runtime binding is not authoritative")?;
        (provider, tab.agent_session_id.clone(), nonce)
    };
    let mut result = latest(svc, &provider, None)?;
    let readings = USAGE.lock();
    let reading = readings.get(tab_id).filter(|reading| {
        reading.provider == provider
            && reading.nonce == nonce
            && (reading.thread_id == thread_id || reading.thread_id.is_none())
    });
    if let Some(reading) = reading {
        result["runtime_usage"] = serde_json::json!([reading]);
    } else if let Some(rows) = result
        .get_mut("runtime_usage")
        .and_then(|rows| rows.as_array_mut())
    {
        rows.retain(|row| row.get("tab_id").and_then(|value| value.as_str()) == Some(tab_id));
    }
    if result.is_null() {
        result = serde_json::json!({"provider":provider,"limits_reported":false});
    }
    Ok(result)
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
mod tests;
