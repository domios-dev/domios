use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::probe::UsageProbe;
use super::store::{UsageFailureKind, UsageStoreHandle};
use crate::agent::usage_parser::UsageParseOutcome;
use crate::service::ServiceContext;

const TICK_INTERVAL: Duration = Duration::from_secs(30);
/// Adaptive backoff steps in seconds: 10 min at peak activity, up to 50 min
/// when idle. Last value is the floor for overnight off-device usage capture.
const BACKOFF_STEPS_S: [u64; 5] = [600, 1200, 1800, 2400, 3000];
/// When the latest snapshot's reset timer has already passed, a new cycle began
/// and the stored pct is stale — probe promptly instead of waiting out the
/// 10-50 min backoff. This floor keeps a reset that never lands a fresh future
/// timer from hammering the provider every tick.
const RESET_PROBE_MIN_INTERVAL: Duration = Duration::from_secs(120);

#[derive(Clone, Default)]
pub struct TabActivity {
    inner: Arc<Mutex<HashMap<String, Instant>>>,
}

impl TabActivity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark(&self, tab_id: &str) {
        self.inner.lock().insert(tab_id.to_string(), Instant::now());
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
}

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
        tauri::async_runtime::spawn(async move {
            loop {
                if let Err(e) = tick(&inner).await {
                    log::warn!("[usage.poller] tick error: {}", e);
                }
                tokio::time::sleep(TICK_INTERVAL).await;
            }
        });
    }

    pub fn refresh_now(&self, provider: &str, account_id: Option<String>) {
        if !crate::config::usage_tracking_enabled(provider) {
            log::info!(
                "[usage.poller] manual refresh skipped — tracking disabled for provider {}",
                provider
            );
            return;
        }
        let key = AccountKey {
            provider: provider.to_string(),
            account_id,
        };
        // Skip spawn if a probe is already running. run_probe_for is the definitive guard, but checking here avoids spawning a task that would immediately return.
        if self
            .inner
            .accounts
            .lock()
            .get(&key)
            .map(|s| s.in_flight)
            .unwrap_or(false)
        {
            return;
        }
        let inner = self.inner.clone();
        tauri::async_runtime::spawn(async move {
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
        let fetch_based = provider_is_fetch_based(&inner.svc, provider);
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
        if let Err(e) = run_probe_for(inner.clone(), key.clone()).await {
            log::warn!(
                "[usage.poller] probe {:?}/{:?} failed: {}",
                key.provider,
                key.account_id,
                e
            );
        }
    }
    Ok(())
}

fn activity_gate(
    inner: &Arc<UsagePollerInner>,
    key: &AccountKey,
    now: Instant,
    reset_due: bool,
) -> bool {
    let mut accounts = inner.accounts.lock();
    let state = accounts.entry(key.clone()).or_default();
    if state.in_flight {
        return false;
    }
    // fresh cycle shows up promptly; otherwise honour the adaptive backoff.
    let interval = if reset_due {
        RESET_PROBE_MIN_INTERVAL
    } else {
        current_backoff(state.backoff_idx)
    };
    if let Some(last) = state.last_probe_at {
        if now.duration_since(last) < interval {
            return false;
        }
    }
    // Gate is time-only here; the step adjustment (reset vs climb) happens in on_success/on_failure once the probe completes.
    true
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
        if state.in_flight {
            return Ok(());
        }
        state.in_flight = true;
        state.last_probe_at = Some(Instant::now());
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
                    match inner.store.insert_snapshot(&snap) {
                        Ok(id) => {
                            crate::usage::projection::invalidate_cache(
                                &key.provider,
                                key.account_id.as_deref(),
                            );
                            on_success(&inner, &key);
                            emit_event(&inner.svc, &key, id);
                            if snap.session_pct.is_some_and(|pct| pct >= 100.0)
                                || snap.weekly_pct.is_some_and(|pct| pct >= 100.0)
                                || snap
                                    .spend_remaining_cents
                                    .is_some_and(|remaining| remaining <= 0.0)
                            {
                                inner.svc.signal_provider_exhaustion(&key.provider);
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
            let _ = inner.store.insert_failure_kind(
                &key.provider,
                key.account_id.as_deref(),
                None,
                UsageFailureKind::Timeout,
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
    let last_probe = state.last_probe_at;
    drop(accounts);
    // Adaptive backoff: reset to densest if terminal was active since the previous probe; otherwise climb one step (capped at the longest).
    let tabs = visible_tabs_for_provider(&inner.svc, &key.provider);
    let active_recently = match last_probe {
        Some(probe_at) => tabs.iter().any(|p| {
            inner
                .activity
                .get(p)
                .map(|act| act > probe_at)
                .unwrap_or(false)
        }),
        None => true,
    };
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
    // Climb one backoff step on every failure; stays at max (50 min) indefinitely. No hard pause — the max-step interval is the rate limit.
    if state.backoff_idx + 1 < BACKOFF_STEPS_S.len() {
        state.backoff_idx += 1;
    }
    let failures = state.consecutive_failures;
    drop(accounts);

    crate::logs::record(
        crate::logs::LogLevel::Warn,
        &format!("usage.parser.{}", key.provider),
        format!("probe failed (#{}) · {}", failures, reason),
        Some(serde_json::json!({
            "provider": key.provider,
            "account_id": key.account_id,
            "failures": failures,
            "reason": reason,
        })),
    );
}

fn providers_with_parser(svc: &ServiceContext) -> Vec<String> {
    svc.agent_registry
        .provider_ids()
        .into_iter()
        .filter(|id| {
            svc.agent_registry
                .with_provider(id, |p| p.usage_parser().is_some())
                .unwrap_or(false)
        })
        .filter(|id| crate::config::usage_tracking_enabled(id))
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

fn probe_command_for(svc: &ServiceContext, provider: &str) -> Option<String> {
    svc.agent_registry
        .with_provider(provider, |p| p.usage_probe_command().map(str::to_string))
        .flatten()
}

fn visible_tabs_for_provider(svc: &ServiceContext, provider: &str) -> Vec<String> {
    let mux = svc.mux.write();
    let snap = mux.snapshot();
    snap.tabs
        .values()
        .filter(|p| !p.hidden && p.agent_type.as_deref() == Some(provider) && p.pty_id.is_some())
        .map(|p| p.id.clone())
        .collect()
}

fn accounts_for_tabs(_svc: &ServiceContext, _tabs: &[String]) -> Vec<Option<String>> {
    vec![None]
}

fn emit_event(svc: &ServiceContext, key: &AccountKey, snapshot_id: i64) {
    let payload = serde_json::json!({
        "provider": key.provider,
        "account_id": key.account_id,
        "snapshot_id": snapshot_id,
    });
    crate::api::events::emit_everywhere(&svc.app_handle, "usage-snapshot-updated", &payload);
    svc.events.broadcast("usage-snapshot-updated", payload);
}

#[cfg(test)]
mod tests {
    use super::row_reset_due;
    use crate::usage::store::UsageSnapshotRow;

    fn row(session: Option<i64>, weekly: Option<i64>) -> UsageSnapshotRow {
        UsageSnapshotRow {
            id: 1,
            provider: "claude".into(),
            account_id: None,
            captured_at: 0,
            parsed_ok: true,
            session_pct: Some(90.0),
            session_resets_at: session,
            weekly_pct: Some(50.0),
            weekly_resets_at: weekly,
            spend_cents: None,
            spend_limit_cents: None,
            spend_remaining_cents: None,
            spend_resets_at: None,
            failure_reason: None,
            probe_command: None,
            raw_excerpt: None,
            debug_json: None,
        }
    }

    fn row_with_spend(spend: Option<i64>) -> UsageSnapshotRow {
        UsageSnapshotRow {
            spend_resets_at: spend,
            ..row(None, None)
        }
    }

    #[test]
    fn reset_not_due_when_timers_in_future() {
        let now = 1_000_000;
        assert!(!row_reset_due(&row(Some(now + 1), Some(now + 5)), now));
    }

    #[test]
    fn reset_due_when_session_timer_passed() {
        let now = 1_000_000;
        assert!(row_reset_due(&row(Some(now - 1), Some(now + 5)), now));
    }

    #[test]
    fn reset_due_when_weekly_timer_passed() {
        let now = 1_000_000;
        assert!(row_reset_due(&row(Some(now + 5), Some(now)), now));
    }

    #[test]
    fn reset_due_when_spend_timer_passed() {
        let now = 1_000_000;
        assert!(row_reset_due(&row_with_spend(Some(now)), now));
    }

    #[test]
    fn reset_not_due_when_no_timers() {
        assert!(!row_reset_due(&row(None, None), 1_000_000));
    }

    /// Falsification oracle for "the usage poller reads the meter through a
    /// path that consumes the meter". A provider with no quota-free usage
    /// source has only the metered agent-spawn path left, so scheduling it at
    /// all is the defect — a visible tab does not make the read free.
    #[test]
    fn provider_without_quota_free_source_is_never_scheduled() {
        assert!(
            !super::provider_pollable(false, false),
            "no quota-free usage source → must not be scheduled, even with a visible Tab"
        );
        assert!(
            !super::provider_pollable(true, false),
            "no quota-free usage source and no Tab → must not be scheduled"
        );
    }

    #[test]
    fn fetch_provider_polls_without_a_tab() {
        assert!(
            super::provider_pollable(true, true),
            "fetch-based provider needs no terminal → poll even with no Tab"
        );
        assert!(
            super::provider_pollable(false, true),
            "Tab present → still poll"
        );
    }
}
