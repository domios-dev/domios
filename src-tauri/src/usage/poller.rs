use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::Emitter;

use super::probe::UsageProbe;
use super::store::UsageStoreHandle;
use crate::service::ServiceContext;

const TICK_INTERVAL: Duration = Duration::from_secs(30);
/// Adaptive backoff steps in seconds: 10 min at peak activity, up to 50 min
/// when idle. Last value is the floor for overnight off-device usage capture.
const BACKOFF_STEPS_S: [u64; 5] = [600, 1200, 1800, 2400, 3000];
/// Probe attempts per poll. A single ConPTY hiccup or a slow first `/usage`
/// load shouldn't burn the whole (10-50 min) interval — respawn a fresh hidden
/// pane and try once more before recording a failure. Permanent errors (missing
/// binary) skip the retry. Each attempt tears down its own pane, so there is no
/// leak across retries.
const PROBE_MAX_ATTEMPTS: u32 = 2;

#[derive(Clone, Default)]
pub struct PaneActivity {
    inner: Arc<Mutex<HashMap<String, Instant>>>,
}

impl PaneActivity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark(&self, pane_id: &str) {
        self.inner
            .lock()
            .insert(pane_id.to_string(), Instant::now());
    }

    pub fn forget(&self, pane_id: &str) {
        self.inner.lock().remove(pane_id);
    }

    pub fn get(&self, pane_id: &str) -> Option<Instant> {
        self.inner.lock().get(pane_id).copied()
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
    activity: PaneActivity,
    accounts: Mutex<HashMap<AccountKey, AccountState>>,
    force_refresh: Mutex<Vec<AccountKey>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AccountKey {
    provider: String,
    account_id: Option<String>,
}

impl UsagePoller {
    pub fn new(svc: ServiceContext, store: UsageStoreHandle, activity: PaneActivity) -> Self {
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
    let mut candidates: Vec<AccountKey> = Vec::new();

    for provider in &providers {
        let visible_panes = visible_panes_for_provider(&inner.svc, provider);
        if visible_panes.is_empty() {
            continue;
        }
        let accounts: Vec<Option<String>> = accounts_for_panes(&inner.svc, &visible_panes);
        for acct in accounts {
            let key = AccountKey {
                provider: provider.clone(),
                account_id: acct,
            };
            if forced.contains(&key) {
                candidates.push(key);
                continue;
            }
            if !activity_gate(inner, &visible_panes, &key, now) {
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
    panes: &[String],
    key: &AccountKey,
    now: Instant,
) -> bool {
    let mut accounts = inner.accounts.lock();
    let state = accounts.entry(key.clone()).or_default();
    if state.in_flight {
        return false;
    }
    let interval = current_backoff(state.backoff_idx);
    if let Some(last) = state.last_probe_at {
        if now.duration_since(last) < interval {
            return false;
        }
    }
    // Gate is time-only here; the step adjustment (reset vs climb) happens in on_success/on_failure once the probe completes.
    let _ = panes;
    true
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

/// Run the probe, respawning a fresh hidden pane and retrying on a recoverable
/// failure (transient ConPTY race, slow load, no output). A missing binary is
/// permanent and skips the retry. The final error — if every attempt fails — is
/// returned so the caller records an HONEST failure rather than hanging.
async fn run_probe_with_retry(
    svc: &ServiceContext,
    key: &AccountKey,
) -> Result<super::probe::UsageProbeOutcome, super::probe::ProbeError> {
    use super::probe::ProbeError;
    let mut attempt = 1;
    loop {
        let probe = UsageProbe::new(svc.clone());
        match probe.run(&key.provider, key.account_id.as_deref()).await {
            Ok(o) => return Ok(o),
            Err(e) => {
                let recoverable = !matches!(e, ProbeError::BinaryMissing(_));
                if !recoverable || attempt >= PROBE_MAX_ATTEMPTS {
                    return Err(e);
                }
                log::warn!(
                    "[usage.poller] probe {:?} attempt {}/{} failed ({}); retrying with fresh pane",
                    key.provider,
                    attempt,
                    PROBE_MAX_ATTEMPTS,
                    e
                );
                attempt += 1;
            }
        }
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

    let outcome = run_probe_with_retry(&inner.svc, &key).await;

    match outcome {
        Ok(o) => {
            let parsed = inner
                .svc
                .agent_registry
                .with_provider(&key.provider, |p| {
                    p.usage_parser().map(|parser| parser.parse(&o.raw_text))
                })
                .flatten();
            match parsed {
                Some(Ok(mut snap)) => {
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
                        }
                        Err(e) => {
                            log::warn!("[usage.poller] db insert: {}", e);
                            on_failure(&inner, &key, "db_insert");
                            let _ = inner.store.insert_failure(
                                &key.provider,
                                key.account_id.as_deref(),
                                Some(&o.raw_text),
                                &format!("db: {}", e),
                            );
                        }
                    }
                }
                Some(Err(e)) => {
                    let msg = e.to_string();
                    let _ = inner.store.insert_failure(
                        &key.provider,
                        key.account_id.as_deref(),
                        Some(&o.raw_text),
                        &msg,
                    );
                    on_failure(&inner, &key, &msg);
                }
                None => {
                    on_failure(&inner, &key, "no_parser");
                }
            }
        }
        Err(e) => {
            let msg = e.to_string();
            let _ =
                inner
                    .store
                    .insert_failure(&key.provider, key.account_id.as_deref(), None, &msg);
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
    let panes = visible_panes_for_provider(&inner.svc, &key.provider);
    let active_recently = match last_probe {
        Some(probe_at) => panes.iter().any(|p| {
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

fn visible_panes_for_provider(svc: &ServiceContext, provider: &str) -> Vec<String> {
    let mux = svc.mux.write();
    let snap = mux.snapshot();
    snap.panes
        .values()
        .filter(|p| !p.hidden && p.agent_type.as_deref() == Some(provider) && p.pty_id.is_some())
        .map(|p| p.id.clone())
        .collect()
}

fn accounts_for_panes(_svc: &ServiceContext, _panes: &[String]) -> Vec<Option<String>> {
    vec![None]
}

fn emit_event(svc: &ServiceContext, key: &AccountKey, snapshot_id: i64) {
    let payload = serde_json::json!({
        "provider": key.provider,
        "account_id": key.account_id,
        "snapshot_id": snapshot_id,
    });
    let _ = svc
        .app_handle
        .emit("usage-snapshot-updated", payload.clone());
    svc.events.broadcast("usage-snapshot-updated", payload);
}
