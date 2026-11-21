use super::{engine::Session, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    RuntimeAccountCredits, RuntimeContext, RuntimeRateLimit, RuntimeSettings, TokenUsage,
};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    sync::LazyLock,
};

mod limits;
pub mod quota_feed;
mod tab_usage;
pub use quota_feed::{ingest_statusline_quota, observe_rate_limit_reset};
pub use tab_usage::{for_tab, read_for_tab};

use codeterm_plugin::api_contracts::Reading;

use codeterm_plugin::api_contracts::UsageProjection;

const MAX_RECENT_TURNS: usize = 64;

struct TurnUsageTotals {
    provider: String,
    account_id: Option<String>,
    nonce: String,
    thread_id: Option<String>,
    settled: Option<TokenUsage>,
    recent: VecDeque<(String, TokenUsage)>,
}

static USAGE: LazyLock<Mutex<HashMap<String, UsageProjection>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static TURN_USAGE: LazyLock<Mutex<HashMap<String, TurnUsageTotals>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn max_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn add_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (a, b) => a.or(b),
    }
}

fn max_token_usage(held: &TokenUsage, next: &TokenUsage) -> TokenUsage {
    TokenUsage {
        response_id: next
            .response_id
            .clone()
            .or_else(|| held.response_id.clone()),
        cost_micros: max_opt(held.cost_micros, next.cost_micros),
        input_tokens: held.input_tokens.max(next.input_tokens),
        output_tokens: held.output_tokens.max(next.output_tokens),
        cache_read_tokens: max_opt(held.cache_read_tokens, next.cache_read_tokens),
        cache_write_tokens: max_opt(held.cache_write_tokens, next.cache_write_tokens),
        used_tokens: next.used_tokens.or(held.used_tokens),
        window_tokens: next.window_tokens.or(held.window_tokens),
    }
}

fn sum_token_usage(base: &TokenUsage, add: &TokenUsage) -> TokenUsage {
    TokenUsage {
        response_id: add.response_id.clone().or_else(|| base.response_id.clone()),
        cost_micros: add_opt(base.cost_micros, add.cost_micros),
        input_tokens: base.input_tokens.saturating_add(add.input_tokens),
        output_tokens: base.output_tokens.saturating_add(add.output_tokens),
        cache_read_tokens: add_opt(base.cache_read_tokens, add.cache_read_tokens),
        cache_write_tokens: add_opt(base.cache_write_tokens, add.cache_write_tokens),
        used_tokens: add.used_tokens.or(base.used_tokens),
        window_tokens: add.window_tokens.or(base.window_tokens),
    }
}

fn accumulate_runtime_turn_usage(
    provider: &str,
    context: &RuntimeContext,
    account: Option<&str>,
    usage: TokenUsage,
) -> TokenUsage {
    let Some(response_id) = usage
        .response_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
    else {
        return usage;
    };
    let mut totals = TURN_USAGE.lock();
    let entry = totals
        .entry(context.tab_id.clone())
        .or_insert_with(|| TurnUsageTotals {
            provider: provider.into(),
            account_id: account.map(str::to_owned),
            nonce: context.binding_nonce.clone(),
            thread_id: context.thread_id.clone(),
            settled: None,
            recent: VecDeque::new(),
        });
    if entry.provider != provider
        || entry.account_id.as_deref() != account
        || entry.nonce != context.binding_nonce
        || entry.thread_id != context.thread_id
    {
        entry.provider = provider.into();
        entry.account_id = account.map(str::to_owned);
        entry.nonce = context.binding_nonce.clone();
        entry.thread_id = context.thread_id.clone();
        entry.settled = None;
        entry.recent.clear();
    }
    if let Some((_, existing)) = entry.recent.iter_mut().find(|(id, _)| id == &response_id) {
        *existing = max_token_usage(existing, &usage);
    } else {
        if entry.recent.len() >= MAX_RECENT_TURNS {
            if let Some((_, oldest)) = entry.recent.pop_front() {
                entry.settled = Some(match &entry.settled {
                    Some(base) => sum_token_usage(base, &oldest),
                    None => oldest,
                });
            }
        }
        entry.recent.push_back((response_id, usage));
    }
    let mut total = entry.settled.clone();
    for (_, item) in &entry.recent {
        total = Some(match total {
            Some(base) => sum_token_usage(&base, item),
            None => item.clone(),
        });
    }
    total.expect("at least one turn usage entry was inserted")
}

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
    account: Option<&str>,
) -> &'a mut UsageProjection {
    let reading = readings
        .entry(context.tab_id.clone())
        .or_insert_with(|| UsageProjection {
            tab_id: context.tab_id.clone(),
            thread_id: context.thread_id.clone(),
            account_id: account.map(str::to_owned),
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
    if reading.provider != provider
        || reading.nonce != context.binding_nonce
        || reading.account_id.as_deref() != account
    {
        reading.provider = provider.into();
        reading.account_id = account.map(str::to_owned);
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

fn selected_account(session: &Session) -> Option<String> {
    let account = session.account.lock();
    (account.as_str() != codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT)
        .then(|| account.clone())
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
    let account = selected_account(session);
    let mut readings = USAGE.lock();
    let reading = projection(
        &mut readings,
        &session.provider,
        &context,
        account.as_deref(),
    );
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
    if mode == codeterm_usage::contract::BillingMode::Unknown {
        TURN_USAGE.lock().remove(&context.tab_id);
    }
    svc.notify_event(
        "usage-snapshot-updated",
        serde_json::json!(codeterm_usage::contract::UsageSnapshotEvent::Billing {
            provider: &session.provider,
            account_id: account.as_deref(),
            tab_id: &context.tab_id,
            captured_at: at
        }),
    );
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
    let context = session.context.lock().clone();
    if let Some(thread_id) = &context.thread_id {
        svc.record_kpi_runtime_usage(&context.tab_id, &session.provider, thread_id, &usage, at);
    }
    if !publishes_usage(svc, session) {
        return;
    }
    let account = selected_account(session);
    let runtime_turn = svc
        .agent_registry
        .with_provider(&session.provider, |provider| provider.usage_accounting())
        == Some(codeterm_plugin::token_usage::UsageAccounting::RuntimeTurn);
    let projected = if runtime_turn {
        accumulate_runtime_turn_usage(&session.provider, &context, account.as_deref(), usage)
    } else {
        usage
    };
    let mut readings = USAGE.lock();
    let reading = projection(
        &mut readings,
        &session.provider,
        &context,
        account.as_deref(),
    );
    reading.captured_at = at;
    reading.token_usage = Some(Reading {
        captured_at: at,
        value: projected,
    });
    drop(readings);
    svc.notify_event(
        "usage-snapshot-updated",
        serde_json::json!(codeterm_usage::contract::UsageSnapshotEvent::Runtime {
            provider: &session.provider,
            account_id: account.as_deref()
        }),
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
    let account = selected_account(session);
    let reported = rate_sample(&session.provider, &limits, at);
    let merged = {
        let mut readings = USAGE.lock();
        let reading = projection(
            &mut readings,
            &session.provider,
            &context,
            account.as_deref(),
        );
        limits::merge(reading.rate_limits.clone(), limits.clone(), at)
            .map_err(|error| RuntimeFailure::InvalidFrame(error.message))?
    };
    let mut sample = rate_sample(&session.provider, &merged.value, at);
    sample.account_id = account.clone();
    sample.session_resets_at_ms = reported.session_resets_at_ms;
    sample.weekly_resets_at_ms = reported.weekly_resets_at_ms;
    let store = svc.usage_store.clone();
    let snapshot_id = if merged.value.is_empty() {
        None
    } else {
        Some(
            tokio::task::spawn_blocking(move || store.insert_runtime_snapshot(&sample))
                .await
                .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
                .map_err(RuntimeFailure::HookFailed)?,
        )
    };
    {
        let mut readings = USAGE.lock();
        let reading = projection(
            &mut readings,
            &session.provider,
            &context,
            account.as_deref(),
        );
        let merged = limits::merge(reading.rate_limits.clone(), limits, at)
            .map_err(|error| RuntimeFailure::InvalidFrame(error.message))?;
        reading.captured_at = reading.captured_at.max(at);
        reading.resets = if supported { resets } else { None };
        reading.account_credits = account_credits;
        reading.rate_limits = (!merged.value.is_empty()).then_some(merged);
    }
    let event = match snapshot_id {
        Some(snapshot_id) => codeterm_usage::contract::UsageSnapshotEvent::Snapshot {
            provider: &session.provider,
            account_id: account.as_deref(),
            snapshot_id,
        },
        None => codeterm_usage::contract::UsageSnapshotEvent::Runtime {
            provider: &session.provider,
            account_id: account.as_deref(),
        },
    };
    svc.notify_event("usage-snapshot-updated", serde_json::json!(event));
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
    let named = limits
        .iter()
        .all(|window| matches!(window.window.as_str(), "primary" | "secondary"));
    let session = if named {
        limits.iter().find(|window| window.window == "primary")
    } else if highest == longest {
        shortest
    } else {
        highest
    };
    let weekly = if named {
        limits.iter().find(|window| window.window == "secondary")
    } else {
        longest.filter(|window| Some(*window) != session)
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
        weekly_pct: weekly.map(|window| window.used_percent as f32),
        weekly_resets_at_ms: weekly
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
        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let row = quota_feed::apply_quota_reset(
            svc.usage_store.get_latest_or_failure(provider, account)?,
            provider,
            account,
            now_ms,
        );
        return enrich_latest(row, provider, vec![]);
    }
    let row = svc
        .usage_store
        .get_latest_with_stale_reason(provider, account)?;
    // A profile's plugin-read balance is its own reading, never the provider's account meter.
    let row = row.filter(|row| {
        row.account_id
            .as_deref()
            .and_then(
                codeterm_plugin::agent_runtime::limits_source::profile_from_balance_account_key,
            )
            .is_none()
    });
    let mux = svc.mux.read();
    let mut readings = USAGE.lock();
    readings.retain(|tab, _| mux.get_tab(tab).is_some());
    let mut current = readings
        .values()
        .filter(|reading| {
            reading.provider == provider
                && reading.account_id.as_deref() == account
                && mux.get_tab(&reading.tab_id).is_some_and(|tab| {
                    tab.agent_type.as_deref() == Some(provider)
                        && crate::service::provider_accounts::selected_account(tab)
                            == account.unwrap_or(
                                codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT,
                            )
                        && (tab.agent_session_id == reading.thread_id
                            || reading.thread_id.is_none())
                        && tab.session_binding_nonce.as_deref() == Some(&reading.nonce)
                        && tab.reported_session_binding_nonce.as_deref() == Some(&reading.nonce)
                })
        })
        .cloned()
        .map(|mut reading| {
            if let Some(limits) = &mut reading.rate_limits {
                limits::expire(limits, chrono::Utc::now().timestamp_millis() as u64);
            }
            if reading.thread_id.is_none() {
                reading.thread_id = mux
                    .get_tab(&reading.tab_id)
                    .and_then(|tab| tab.agent_session_id.clone());
            }
            reading
        })
        .collect::<Vec<_>>();
    drop(readings);
    TURN_USAGE
        .lock()
        .retain(|tab, _| mux.get_tab(tab).is_some());
    drop(mux);
    current.sort_by(|a, b| a.tab_id.cmp(&b.tab_id));
    enrich_latest(row, provider, current)
}

fn enrich_latest(
    row: Option<codeterm_usage::contract::UsageSnapshotRow>,
    provider: &str,
    current: Vec<UsageProjection>,
) -> Result<serde_json::Value, String> {
    use codeterm_plugin::api_contracts::UsageLatestResponse;
    let result = match row {
        Some(snapshot) => Some(UsageLatestResponse::Stored {
            snapshot,
            runtime_usage: (!current.is_empty()).then_some(current),
        }),
        None if current.is_empty() => None,
        None => Some(UsageLatestResponse::RuntimeOnly {
            provider: provider.to_owned(),
            limits_reported: false,
            runtime_usage: current,
        }),
    };
    serde_json::to_value(result).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
