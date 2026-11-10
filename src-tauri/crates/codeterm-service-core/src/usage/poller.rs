use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_usage::UsageSample;
use parking_lot::Mutex;

use super::probe::UsageProbe;
use super::store::{UsageFailureKind, UsageStoreHandle};
use crate::agent::usage_parser::{UsageParseOutcome, UsageSnapshot};
use crate::service::ServiceContext;

const TICK_INTERVAL: Duration = Duration::from_secs(30);
/// A single >=100 telemetry reading can be a transient blip; treat the provider
/// as exhausted only after this many consecutive confirming snapshots.
const EXHAUSTION_CONFIRM_THRESHOLD: u32 = 3;
/// Adaptive backoff steps in seconds: 10 min at peak activity, up to 50 min
/// when idle. Last value is the floor for overnight off-device usage capture.
const BACKOFF_STEPS_S: [u64; 5] = [600, 1200, 1800, 2400, 3000];
/// When the latest snapshot's reset timer has already passed, a new cycle began
/// and the stored pct is stale — probe promptly instead of waiting out the
/// 10-50 min backoff. This floor keeps a reset that never lands a fresh future
/// timer from hammering the provider every tick.
const RESET_PROBE_MIN_INTERVAL: Duration = Duration::from_secs(120);

pub(super) fn usage_sample(snapshot: &UsageSnapshot) -> UsageSample {
    UsageSample {
        usage_details: snapshot.usage_details.clone(),
        provider: snapshot.provider.clone(),
        billing_mode: snapshot.billing_mode,
        limits_reported: snapshot.limits_reported,
        account_id: snapshot.account_id.clone(),
        captured_at_ms: snapshot.captured_at_ms,
        session_pct: snapshot.session_pct,
        session_resets_at_ms: snapshot.session_resets_at_ms,
        weekly_pct: snapshot.weekly_pct,
        weekly_resets_at_ms: snapshot.weekly_resets_at_ms,
        spend_cents: snapshot.spend_cents,
        spend_limit_cents: snapshot.spend_limit_cents,
        spend_remaining_cents: snapshot.spend_remaining_cents,
        spend_resets_at_ms: snapshot.spend_resets_at_ms,
        raw_text: snapshot.raw_text.clone(),
    }
}

/// Classify a probe failure. A confined plugin that was denied a capability is
/// its own kind: nothing upstream is broken, the provider simply cannot reach
/// what it needs, and that is the state a frozen gauge has to be able to name.
fn failure_kind_for(error: &super::probe::ProbeError) -> UsageFailureKind {
    match error {
        super::probe::ProbeError::FetchFailed(detail)
            if crate::plugin::provider::is_capability_denied(detail) =>
        {
            UsageFailureKind::CapabilityDenied
        }
        super::probe::ProbeError::FetchFailed(_) => UsageFailureKind::FetchError,
        super::probe::ProbeError::NoQuotaFreeSource(_) => UsageFailureKind::SourceUnavailable,
        super::probe::ProbeError::Internal(_) => UsageFailureKind::Internal,
        super::probe::ProbeError::BinaryMissing(_) => UsageFailureKind::SourceUnavailable,
    }
}

#[derive(Clone, Default)]
pub struct TabActivity {
    inner: Arc<Mutex<HashMap<String, Instant>>>,
    changed: Arc<tokio::sync::Notify>,
}

impl TabActivity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark(&self, tab_id: &str) {
        let now = Instant::now();
        let previous = self.inner.lock().insert(tab_id.to_string(), now);
        if previous.is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(300)) {
            self.changed.notify_one();
        }
    }

    pub fn forget(&self, tab_id: &str) {
        self.inner.lock().remove(tab_id);
    }

    pub fn get(&self, tab_id: &str) -> Option<Instant> {
        self.inner.lock().get(tab_id).copied()
    }
}

#[derive(Default)]
struct AccountState {
    last_probe_at: Option<Instant>,
    last_success_at: Option<Instant>,
    consecutive_failures: u32,
    /// Index into `BACKOFF_STEPS_S` representing the *next* probe's wait.
    /// Resets to 0 (densest) on any terminal activity detected before the
    /// previous probe, climbs by one on each "no activity" tick.
    backoff_idx: usize,
    in_flight: bool,
    /// Consecutive >=100 readings; the debounce that stops one blippy sample
    /// from nuking in-flight workers.
    exhaustion_streak: u32,
    /// True once a confirmed exhaustion signal fired; cleared by a healthy
    /// reading so exhaustion fires once per cycle, not on every tick.
    exhaustion_fired: bool,
}

impl AccountState {
    fn start_probe(&mut self, now: Instant) -> bool {
        if self.in_flight
            || self
                .last_probe_at
                .is_some_and(|at| now.saturating_duration_since(at) < RESET_PROBE_MIN_INTERVAL)
        {
            return false;
        }
        self.in_flight = true;
        self.last_probe_at = Some(now);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExhaustionSignal {
    Confirmed,
    Recovered,
}

pub struct UsagePollerSlot(pub Arc<Mutex<Option<UsagePoller>>>);

#[derive(Clone)]
pub struct UsagePoller {
    inner: Arc<UsagePollerInner>,
}

struct UsagePollerInner {
    svc: ServiceContext,
    store: UsageStoreHandle,
    activity: TabActivity,
    accounts: Mutex<HashMap<AccountKey, AccountState>>,
    force_refresh: Mutex<Vec<AccountKey>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AccountKey {
    provider: String,
    account_id: Option<String>,
}

impl UsagePoller {
    pub fn new(svc: ServiceContext, store: UsageStoreHandle, activity: TabActivity) -> Self {
        Self {
            inner: Arc::new(UsagePollerInner {
                svc,
                store,
                activity,
                accounts: Mutex::new(HashMap::new()),
                force_refresh: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn spawn(self) {
        let inner = self.inner.clone();
        crate::async_runtime::spawn(async move {
            let mut deadline = tokio::time::interval(TICK_INTERVAL);
            deadline.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = deadline.tick() => {},
                    _ = inner.activity.changed.notified() => {},
                }
                if let Err(e) = tick(&inner).await {
                    log::warn!("[usage.poller] tick error: {}", e);
                }
            }
        });
    }

    pub fn refresh_now(&self, provider: &str, account_id: Option<String>) {
        if !crate::config::usage_tracking_enabled_from(&self.inner.svc.configuration(), provider) {
            log::info!(
                "[usage.poller] manual refresh skipped — tracking disabled for provider {}",
                provider
            );
            return;
        }
        if let Some(profile) = account_id.as_deref().and_then(
            codeterm_plugin::agent_runtime::limits_source::profile_from_balance_account_key,
        ) {
            super::profile_limits::spawn_refresh(
                self.inner.svc.clone(),
                self.inner.store.clone(),
                provider.to_string(),
                profile.to_string(),
            );
            return;
        }
        let key = AccountKey {
            provider: provider.to_string(),
            account_id,
        };
        {
            let mut accounts = self.inner.accounts.lock();
            let state = accounts.entry(key.clone()).or_default();
            if state.in_flight {
                return;
            }
            state.consecutive_failures = 0;
        }
        let inner = self.inner.clone();
        crate::async_runtime::spawn(async move {
            if let Err(e) = run_probe_for(inner, key.clone()).await {
                log::warn!(
                    "[usage.poller] manual refresh probe {:?}/{:?} failed: {}",
                    key.provider,
                    key.account_id,
                    e
                );
            }
        });
    }
}

async fn tick(inner: &Arc<UsagePollerInner>) -> Result<(), String> {
    let providers = providers_with_parser(&inner.svc);
    if providers.is_empty() {
        return Ok(());
    }
    let forced: Vec<AccountKey> = std::mem::take(&mut *inner.force_refresh.lock());
    let now = Instant::now();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut candidates: Vec<AccountKey> = Vec::new();

    for provider in &providers {
        let visible_tabs = visible_tabs_for_provider(&inner.svc, provider);
        let fetch_based = provider_is_fetch_based(&inner.svc, provider)
            || provider_reports_groups(&inner.svc, provider);
        if !provider_pollable(visible_tabs.is_empty(), fetch_based) {
            continue;
        }
        let accounts: Vec<Option<String>> = if visible_tabs.is_empty() {
            vec![None]
        } else {
            accounts_for_tabs(&inner.svc, &visible_tabs)
        };
        for acct in accounts {
            let key = AccountKey {
                provider: provider.clone(),
                account_id: acct,
            };
            if forced.contains(&key) {
                candidates.push(key);
                continue;
            }
            let due = reset_due(inner, &key, now_ms);
            if !activity_gate(inner, &key, now, due) {
                continue;
            }
            candidates.push(key);
        }
    }
    for key in forced.into_iter() {
        if !candidates.contains(&key) {
            candidates.push(key);
        }
    }

    candidates.dedup();

    for key in candidates {
        let inner = inner.clone();
        crate::async_runtime::spawn(async move {
            if let Err(e) = run_probe_for(inner, key.clone()).await {
                log::warn!(
                    "[usage.poller] probe {:?}/{:?} failed: {}",
                    key.provider,
                    key.account_id,
                    e
                );
            }
        });
    }
    Ok(())
}

fn activity_gate(
    inner: &Arc<UsagePollerInner>,
    key: &AccountKey,
    now: Instant,
    reset_due: bool,
) -> bool {
    let active = provider_active(inner, &key.provider, now);
    let accounts = inner.accounts.lock();
    let state = accounts.get(key);
    let interval = if reset_due {
        RESET_PROBE_MIN_INTERVAL
    } else if active {
        Duration::from_secs(300)
    } else {
        current_backoff(state.map_or(0, |state| state.backoff_idx))
    };
    let latest_age = inner
        .store
        .get_latest(&key.provider, key.account_id.as_deref())
        .ok()
        .flatten()
        .map(|row| {
            Duration::from_millis(
                chrono::Utc::now()
                    .timestamp_millis()
                    .saturating_sub(row.captured_at)
                    .max(0) as u64,
            )
        })
        .or_else(|| {
            state
                .and_then(|state| state.last_success_at)
                .map(|at| now.saturating_duration_since(at))
        });
    super::freshness::refresh_due(
        active,
        latest_age,
        state
            .and_then(|state| state.last_probe_at)
            .map(|at| now.saturating_duration_since(at)),
        state.is_some_and(|state| state.in_flight),
        state.map_or(0, |state| state.consecutive_failures),
        interval,
    )
}

/// True when the latest stored snapshot's reset timer has
/// already elapsed — the displayed cycle is over, so a fresh probe is overdue.
fn reset_due(inner: &Arc<UsagePollerInner>, key: &AccountKey, now_ms: i64) -> bool {
    match inner
        .store
        .get_latest(&key.provider, key.account_id.as_deref())
    {
        Ok(Some(row)) => row_reset_due(&row, now_ms),
        _ => false,
    }
}

/// Pure reset-boundary test: any non-null reset timestamp at or before `now_ms`.
fn row_reset_due(row: &super::store::UsageSnapshotRow, now_ms: i64) -> bool {
    let passed = |r: Option<i64>| r.map(|r| r <= now_ms).unwrap_or(false);
    passed(row.session_resets_at) || passed(row.weekly_resets_at) || passed(row.spend_resets_at)
}

/// Returns the wait duration for the given step. Top steps get ±10% jitter so
/// the polling cadence isn't a metronome.
fn current_backoff(idx: usize) -> Duration {
    let clamped = idx.min(BACKOFF_STEPS_S.len() - 1);
    let secs = BACKOFF_STEPS_S[clamped];
    if clamped >= 3 {
        // Cheap deterministic-ish jitter from nanos — no extra dep needed.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        let jitter_range = (secs / 10) * 2;
        let offset = (nanos % jitter_range) as i64 - (jitter_range / 2) as i64;
        Duration::from_secs((secs as i64 + offset).max(60) as u64)
    } else {
        Duration::from_secs(secs)
    }
}

async fn run_probe_for(inner: Arc<UsagePollerInner>, key: AccountKey) -> Result<(), String> {
    {
        let mut accounts = inner.accounts.lock();
        let state = accounts.entry(key.clone()).or_default();
        if !state.start_probe(Instant::now()) {
            return Ok(());
        }
    }

    if !provider_is_fetch_based(&inner.svc, &key.provider)
        && provider_reports_groups(&inner.svc, &key.provider)
    {
        let svc = inner.svc.clone();
        let provider = key.provider.clone();
        let fetched = tokio::task::spawn_blocking(move || {
            svc.agent_registry
                .with_provider(&provider, |p| p.model_group_usage())
        })
        .await;
        match fetched {
            Ok(Some(_)) => {
                on_success(&inner, &key);
                emit_event(&inner.svc, &key, 0);
            }
            other => on_failure(&inner, &key, &format!("group usage unavailable: {other:?}")),
        }
        if let Some(state) = inner.accounts.lock().get_mut(&key) {
            state.in_flight = false;
        }
        return Ok(());
    }

    let outcome = UsageProbe::new(inner.svc.clone())
        .run(&key.provider, key.account_id.as_deref())
        .await;
    let probe_command = probe_command_for(&inner.svc, &key.provider);

    match outcome {
        Ok(o) => {
            let parsed = inner
                .svc
                .agent_registry
                .with_provider(&key.provider, |p| {
                    p.usage_parser()
                        .map(|parser| parser.parse_outcome(&o.raw_text))
                })
                .flatten();
            match parsed {
                Some(Ok(UsageParseOutcome::Parsed(mut snap))) => {
                    snap.account_id = key.account_id.clone();
                    snap.provider = key.provider.clone();
                    match inner.store.insert_snapshot(&usage_sample(&snap)) {
                        Ok(id) => {
                            crate::usage::projection::invalidate_cache(
                                &key.provider,
                                key.account_id.as_deref(),
                            );
                            on_success(&inner, &key);
                            emit_event(&inner.svc, &key, id);
                            let exhausted = snapshot_is_exhausted(&snap);
                            match register_exhaustion_reading(&inner, &key, exhausted) {
                                Some(ExhaustionSignal::Confirmed) => {
                                    if let Err(error) =
                                        inner.svc.signal_provider_exhaustion(&key.provider)
                                    {
                                        log::error!("[worker-interruption] signal failed: {error}");
                                    }
                                }
                                Some(ExhaustionSignal::Recovered) => {
                                    if let Err(error) =
                                        inner.svc.signal_provider_recovery(&key.provider)
                                    {
                                        log::error!("[worker-interruption] signal failed: {error}");
                                    }
                                }
                                None => {}
                            }
                        }
                        Err(e) => {
                            log::warn!("[usage.poller] db insert: {}", e);
                            on_failure(&inner, &key, "db_insert");
                            let _ = inner.store.insert_failure_kind(
                                &key.provider,
                                key.account_id.as_deref(),
                                Some(&o.raw_text),
                                UsageFailureKind::DbInsert,
                                &format!("db: {}", e),
                                probe_command.as_deref(),
                            );
                        }
                    }
                }
                Some(Ok(UsageParseOutcome::Skip(reason))) => {
                    let _ = inner.store.insert_failure_kind(
                        &key.provider,
                        key.account_id.as_deref(),
                        Some(&o.raw_text),
                        UsageFailureKind::Skip,
                        &reason,
                        probe_command.as_deref(),
                    );
                    on_success(&inner, &key);
                }
                Some(Err(e)) => {
                    let msg = e.to_string();
                    let _ = inner.store.insert_failure_kind(
                        &key.provider,
                        key.account_id.as_deref(),
                        Some(&o.raw_text),
                        UsageFailureKind::ParseError,
                        &msg,
                        probe_command.as_deref(),
                    );
                    on_failure(&inner, &key, &msg);
                }
                None => {
                    let _ = inner.store.insert_failure_kind(
                        &key.provider,
                        key.account_id.as_deref(),
                        Some(&o.raw_text),
                        UsageFailureKind::NoParser,
                        "provider has no usage parser",
                        probe_command.as_deref(),
                    );
                    on_failure(&inner, &key, "no_parser");
                }
            }
        }
        Err(e) => {
            let msg = e.to_string();
            let failure_kind = failure_kind_for(&e);
            let _ = inner.store.insert_failure_kind(
                &key.provider,
                key.account_id.as_deref(),
                None,
                failure_kind,
                &msg,
                probe_command.as_deref(),
            );
            on_failure(&inner, &key, &msg);
        }
    }

    let mut accounts = inner.accounts.lock();
    if let Some(state) = accounts.get_mut(&key) {
        state.in_flight = false;
    }
    Ok(())
}

fn on_success(inner: &Arc<UsagePollerInner>, key: &AccountKey) {
    let mut accounts = inner.accounts.lock();
    let state = accounts.entry(key.clone()).or_default();
    state.consecutive_failures = 0;
    state.last_success_at = Some(Instant::now());
    drop(accounts);
    let active_recently = provider_active(inner, &key.provider, Instant::now());
    let mut accounts = inner.accounts.lock();
    let state = accounts.entry(key.clone()).or_default();
    if active_recently {
        state.backoff_idx = 0;
    } else if state.backoff_idx + 1 < BACKOFF_STEPS_S.len() {
        state.backoff_idx += 1;
    }
}

fn on_failure(inner: &Arc<UsagePollerInner>, key: &AccountKey, reason: &str) {
    let mut accounts = inner.accounts.lock();
    let state = accounts.entry(key.clone()).or_default();
    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
    if state.backoff_idx + 1 < BACKOFF_STEPS_S.len() {
        state.backoff_idx += 1;
    }
    let failures = state.consecutive_failures;
    drop(accounts);
    emit_event(&inner.svc, key, 0);

    crate::logs::record(
        crate::logs::LogLevel::Warn,
        &format!("usage.parser.{}", key.provider),
        format!(
            "probe failed (#{}) · {}{}",
            failures,
            reason,
            if failures >= super::freshness::MAX_AUTOMATIC_FAILURES {
                " · automatic probes parked until explicit refresh"
            } else {
                ""
            }
        ),
        Some(serde_json::json!({
            "provider": key.provider,
            "account_id": key.account_id,
            "failures": failures,
            "reason": reason,
        })),
    );
}

/// True when any tracked meter has crossed its limit. A raw reading — the
/// debounce below, not this predicate, decides whether to act on it.
fn snapshot_is_exhausted(snap: &UsageSnapshot) -> bool {
    snap.session_pct.is_some_and(|pct| pct >= 100.0)
        || snap.weekly_pct.is_some_and(|pct| pct >= 100.0)
        || snap
            .spend_remaining_cents
            .is_some_and(|remaining| remaining <= 0.0)
}

/// Pure debounce transition. `Confirmed` fires exactly once, only after
/// `threshold` consecutive exhausted readings; `Recovered` fires once when a
/// healthy reading lands after a confirmed exhaustion. A single blip advances
/// the streak but never crosses the threshold, and any healthy reading resets it.
fn evaluate_exhaustion_debounce(
    streak: &mut u32,
    fired: &mut bool,
    exhausted: bool,
    threshold: u32,
) -> Option<ExhaustionSignal> {
    if exhausted {
        *streak = streak.saturating_add(1);
        if *streak >= threshold && !*fired {
            *fired = true;
            return Some(ExhaustionSignal::Confirmed);
        }
        None
    } else {
        let was_fired = *fired;
        *streak = 0;
        *fired = false;
        was_fired.then_some(ExhaustionSignal::Recovered)
    }
}

fn register_exhaustion_reading(
    inner: &Arc<UsagePollerInner>,
    key: &AccountKey,
    exhausted: bool,
) -> Option<ExhaustionSignal> {
    let mut accounts = inner.accounts.lock();
    let state = accounts.entry(key.clone()).or_default();
    evaluate_exhaustion_debounce(
        &mut state.exhaustion_streak,
        &mut state.exhaustion_fired,
        exhausted,
        EXHAUSTION_CONFIRM_THRESHOLD,
    )
}

fn providers_with_parser(svc: &ServiceContext) -> Vec<String> {
    svc.agent_registry
        .provider_ids()
        .into_iter()
        .filter(|id| {
            svc.agent_registry
                .with_provider(id, |p| {
                    p.usage_parser().is_some() || p.reports_group_usage()
                })
                .unwrap_or(false)
        })
        .filter(|id| crate::config::usage_tracking_enabled_from(&svc.configuration(), id))
        .collect()
}

/// A provider is pollable only when its usage can be read without consuming
/// usage. Tab visibility is deliberately not part of this: an open terminal
/// never makes a read free, it only hides what the read costs.
fn provider_pollable(_visible_empty: bool, fetch_based: bool) -> bool {
    fetch_based
}

fn provider_is_fetch_based(svc: &ServiceContext, provider: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |p| p.usage_is_fetch_based())
        .unwrap_or(false)
}

fn provider_reports_groups(svc: &ServiceContext, provider: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |p| p.reports_group_usage())
        .unwrap_or(false)
}

fn probe_command_for(svc: &ServiceContext, provider: &str) -> Option<String> {
    svc.agent_registry
        .with_provider(provider, |p| p.usage_probe_command().map(str::to_string))
        .flatten()
}

fn visible_tabs_for_provider(svc: &ServiceContext, provider: &str) -> Vec<String> {
    let mux = svc.mux.read();
    let snap = mux.snapshot();
    snap.tabs
        .values()
        .filter(|p| p.agent_type.as_deref() == Some(provider))
        .map(|p| p.id.clone())
        .collect()
}

fn provider_active(inner: &UsagePollerInner, provider: &str, now: Instant) -> bool {
    let tabs = inner.svc.mux.read().snapshot().tabs;
    tabs.values()
        .filter(|tab| tab.agent_type.as_deref() == Some(provider))
        .any(|tab| {
            inner.svc.resolved_tab_status(tab) == Some(codeterm_types::pty::TabStatus::Working)
                || inner
                    .activity
                    .get(&tab.id)
                    .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(300))
        })
}

fn accounts_for_tabs(_svc: &ServiceContext, _tabs: &[String]) -> Vec<Option<String>> {
    vec![None]
}

fn emit_event(svc: &ServiceContext, key: &AccountKey, snapshot_id: i64) {
    emit_snapshot_event(svc, &key.provider, key.account_id.as_deref(), snapshot_id);
}

pub(super) fn emit_snapshot_event(
    svc: &ServiceContext,
    provider: &str,
    account_id: Option<&str>,
    snapshot_id: i64,
) {
    let payload = serde_json::json!(codeterm_usage::contract::UsageSnapshotEvent::Snapshot {
        provider,
        account_id,
        snapshot_id,
    });
    svc.ui.emit(
        "usage-snapshot-updated",
        serde_json::to_value(&payload).unwrap_or_default(),
    );
    svc.events.broadcast("usage-snapshot-updated", payload);
}

#[cfg(test)]
mod tests;
