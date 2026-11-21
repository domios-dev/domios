use super::super::{engine::Session, events::RuntimeEvent};
use crate::service::{agent_runtime::launch_profile, ServiceContext};
use codeterm_plugin::agent_runtime::{
    limits_source, RuntimeErrorSignal, RuntimeRetryableErrorKind,
};
use codeterm_usage::contract::{BillingMode, UsageSnapshotEvent, UsageSnapshotRow};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::LazyLock};

const MAX_QUOTA_BUCKETS: usize = 128;
const MAX_QUOTA_RESETS: usize = 256;
const MAX_RESET_WINDOW_SECS: f64 = 31.0 * 24.0 * 60.0 * 60.0;
const MAX_RESET_WINDOW_MS: u64 = 31 * 24 * 60 * 60 * 1000;
const MAX_SAFE_MS: u64 = 9_007_199_254_740_991;
const MAX_REASON_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedQuotaReset {
    pub resets_at_ms: u64,
    pub observed_at_ms: u64,
    pub reason: Option<String>,
}

static QUOTA_RESETS: LazyLock<Mutex<HashMap<(String, Option<String>), ObservedQuotaReset>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn printable_trimmed(raw: &str, max_chars: usize) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > max_chars
        || trimmed.chars().any(char::is_control)
    {
        return None;
    }
    Some(trimmed.to_owned())
}

fn parse_bucket_reset_at(
    reset_time: Option<&Value>,
    reset_in_seconds: Option<&Value>,
    now_ms: u64,
) -> Option<Option<i64>> {
    if let Some(value) = reset_time.filter(|value| !value.is_null()) {
        let raw = value.as_str()?;
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            if trimmed.len() > 128 {
                return None;
            }
            let parsed = chrono::DateTime::parse_from_rfc3339(trimmed).ok()?;
            return Some(Some(parsed.timestamp_millis()));
        }
    }
    if let Some(value) = reset_in_seconds.filter(|value| !value.is_null()) {
        let secs = value.as_f64()?;
        if !secs.is_finite() || secs < 0.0 || secs > MAX_RESET_WINDOW_SECS {
            return None;
        }
        if secs == 0.0 {
            return Some(None);
        }
        let delta_ms = (secs * 1000.0).round() as u64;
        let resets_at = now_ms.checked_add(delta_ms)?;
        if resets_at > MAX_SAFE_MS {
            return None;
        }
        return Some(Some(resets_at as i64));
    }
    Some(None)
}

fn is_duration_segment(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    bytes.len() >= 2
        && matches!(bytes[bytes.len() - 1], b's' | b'm' | b'h' | b'd' | b'w')
        && bytes[..bytes.len() - 1].iter().all(u8::is_ascii_digit)
}

fn format_bucket_label(raw_key: &str) -> String {
    let parts: Vec<&str> = raw_key
        .trim()
        .split(['-', '_'])
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.is_empty() {
        return raw_key.trim().to_owned();
    }
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let lower = part.to_ascii_lowercase();
            if is_duration_segment(&lower) {
                return lower;
            }
            if index > 0
                && matches!(
                    lower.as_str(),
                    "hourly" | "daily" | "weekly" | "monthly" | "session"
                )
            {
                return lower;
            }
            if let Some(pos) = part
                .find(|c: char| !c.is_ascii_digit())
                .filter(|&pos| pos > 0 && part[pos..].chars().all(|c| c.is_ascii_alphabetic()))
            {
                return format!("{}{}", &part[..pos], part[pos..].to_ascii_uppercase());
            }
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn bucket_sort_meta(raw_key: &str) -> (u8, String, u8) {
    let parts: Vec<&str> = raw_key
        .trim()
        .split(['-', '_'])
        .filter(|segment| !segment.is_empty())
        .collect();
    let group = parts
        .first()
        .copied()
        .unwrap_or_else(|| raw_key.trim())
        .to_ascii_lowercase();
    let digit_group = u8::from(group.as_bytes().first().is_some_and(u8::is_ascii_digit));
    let last = if parts.len() > 1 {
        parts[parts.len() - 1].to_ascii_lowercase()
    } else {
        String::new()
    };
    let last_bytes = last.as_bytes();
    let window_rank = if matches!(last.as_str(), "session" | "hourly")
        || (is_duration_segment(&last)
            && matches!(last_bytes.last().copied(), Some(b's' | b'm' | b'h')))
    {
        0
    } else if last == "daily" || (is_duration_segment(&last) && last_bytes.last() == Some(&b'd')) {
        1
    } else if last == "weekly" || (is_duration_segment(&last) && last_bytes.last() == Some(&b'w')) {
        2
    } else if last == "monthly" {
        3
    } else {
        4
    };
    (digit_group, group, window_rank)
}

/// Parses documented statusLine quota JSON `{ quota: { [bucketId]: { remaining_fraction?, reset_time?, reset_in_seconds?, disabled? } }, plan_tier? }`.
pub fn parse_statusline_quota(body: &Value, now_ms: u64) -> Option<Value> {
    let quota = body.get("quota")?.as_object()?;
    if quota.len() > MAX_QUOTA_BUCKETS {
        return None;
    }
    let mut limits: Vec<(u8, String, u8, String, f64, Option<i64>)> =
        Vec::with_capacity(quota.len());
    for (raw_key, raw_entry) in quota {
        let valid_key = printable_trimmed(raw_key, 128)?;
        let entry = raw_entry.as_object()?;
        if let Some(disabled) = entry.get("disabled") {
            match disabled.as_bool() {
                Some(true) => continue,
                Some(false) => {}
                None => return None,
            }
        }
        let Some(raw_fraction) = entry
            .get("remaining_fraction")
            .filter(|value| !value.is_null())
        else {
            continue;
        };
        let fraction = raw_fraction.as_f64()?;
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return None;
        }
        let resets_at = parse_bucket_reset_at(
            entry.get("reset_time"),
            entry.get("reset_in_seconds"),
            now_ms,
        )?;
        let used_percent = ((1.0 - fraction) * 1000.0).round() / 10.0;
        let (digit_group, group, window_rank) = bucket_sort_meta(&valid_key);
        let label = format_bucket_label(&valid_key);
        limits.push((
            digit_group,
            group,
            window_rank,
            label,
            used_percent,
            resets_at,
        ));
    }
    limits.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| b.4.total_cmp(&a.4))
            .then_with(|| a.3.cmp(&b.3))
    });
    let tier = body
        .get("plan_tier")
        .and_then(Value::as_str)
        .and_then(|value| printable_trimmed(value, 64));
    let title = match tier {
        Some(tier) => format!("Account quota ({tier})"),
        None => String::from("Account quota"),
    };
    let rows = limits
        .into_iter()
        .map(|(_, _, _, label, used_percent, resets_at)| {
            json!({ "label": label, "used_percent": used_percent, "resets_at": resets_at })
        })
        .collect::<Vec<_>>();
    Some(json!({ "title": title, "limits": rows }))
}

fn profile_or_account_key(svc: &ServiceContext, tab_id: &str, session: &Session) -> Option<String> {
    let profile = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(launch_profile::of_tab);
    if let Some(profile) = profile {
        return Some(limits_source::balance_account_key(&profile));
    }
    super::selected_account(session)
}

fn store_quota_reset(
    provider: &str,
    account: Option<String>,
    reset: ObservedQuotaReset,
    now_ms: u64,
) {
    let mut table = QUOTA_RESETS.lock();
    table.retain(|_, entry| entry.resets_at_ms > now_ms);
    if table.len() >= MAX_QUOTA_RESETS {
        if let Some(oldest) = table
            .iter()
            .min_by_key(|(_, entry)| entry.observed_at_ms)
            .map(|(key, _)| key.clone())
        {
            table.remove(&oldest);
        }
    }
    table.insert((provider.to_owned(), account), reset);
}

pub(crate) fn active_quota_reset(
    provider: &str,
    account: Option<&str>,
    now_ms: u64,
) -> Option<ObservedQuotaReset> {
    let key = (provider.to_owned(), account.map(str::to_owned));
    let mut table = QUOTA_RESETS.lock();
    match table.get(&key) {
        Some(entry) if entry.resets_at_ms > now_ms => Some(entry.clone()),
        Some(_) => {
            table.remove(&key);
            None
        }
        None => None,
    }
}

pub(crate) fn clear_quota_reset(provider: &str, account: Option<&str>) {
    QUOTA_RESETS
        .lock()
        .remove(&(provider.to_owned(), account.map(str::to_owned)));
}

pub(crate) fn apply_quota_reset(
    row: Option<UsageSnapshotRow>,
    provider: &str,
    account: Option<&str>,
    now_ms: u64,
) -> Option<UsageSnapshotRow> {
    let Some(reset) = active_quota_reset(provider, account, now_ms) else {
        return row;
    };
    let quota_reset = json!({
        "resets_at": reset.resets_at_ms,
        "observed_at": reset.observed_at_ms,
        "reason": reset.reason,
    });
    match row {
        Some(mut existing) => {
            let mut details = match existing.usage_details.take() {
                Some(Value::Object(map)) => map,
                _ => {
                    let mut map = serde_json::Map::new();
                    map.insert("limits".into(), json!([]));
                    map.insert("dollar_credits".into(), json!([]));
                    map.insert("extra_usage".into(), Value::Null);
                    map
                }
            };
            details.insert("quota_reset".into(), quota_reset);
            existing.usage_details = Some(Value::Object(details));
            Some(existing)
        }
        None => Some(UsageSnapshotRow {
            id: 0,
            usage_details: Some(json!({
                "limits": [],
                "dollar_credits": [],
                "extra_usage": null,
                "quota_reset": quota_reset,
            })),
            provider: provider.to_owned(),
            billing_mode: BillingMode::Subscription,
            limits_reported: false,
            account_id: account.map(str::to_owned),
            captured_at: reset.observed_at_ms as i64,
            parsed_ok: true,
            session_pct: None,
            session_resets_at: None,
            weekly_pct: None,
            weekly_resets_at: None,
            spend_cents: None,
            spend_limit_cents: None,
            spend_remaining_cents: None,
            spend_resets_at: None,
            failure_reason: None,
            probe_command: None,
            raw_excerpt: None,
            debug_json: None,
        }),
    }
}

pub fn observe_rate_limit_reset(
    svc: &ServiceContext,
    session: &Session,
    event: &RuntimeEvent,
    at: u64,
) {
    let RuntimeEvent::ItemUpdated { thread_id, item } = event else {
        return;
    };
    if item.system_subtype.as_deref() != Some("api_error") {
        return;
    }
    let context = session.context.lock().clone();
    if context.thread_id.as_deref() != Some(thread_id.as_str()) {
        return;
    }
    let Some(signal) = item
        .tool_input
        .as_ref()
        .and_then(|value| serde_json::from_value::<RuntimeErrorSignal>(value.clone()).ok())
    else {
        return;
    };
    if signal.retryable_kind != Some(RuntimeRetryableErrorKind::RateLimitedUntil) {
        return;
    }
    let Some(resets_at_ms) = signal.retry_at_ms else {
        return;
    };
    if resets_at_ms <= at
        || resets_at_ms > MAX_SAFE_MS
        || resets_at_ms.saturating_sub(at) > MAX_RESET_WINDOW_MS
    {
        return;
    }
    let account = profile_or_account_key(svc, &context.tab_id, session);
    let normalized = item
        .content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let reason = printable_trimmed(
        &normalized
            .chars()
            .take(MAX_REASON_CHARS)
            .collect::<String>(),
        MAX_REASON_CHARS,
    );
    store_quota_reset(
        &session.provider,
        account.clone(),
        ObservedQuotaReset {
            resets_at_ms,
            observed_at_ms: at,
            reason,
        },
        at,
    );
    svc.notify_event(
        "usage-snapshot-updated",
        json!(UsageSnapshotEvent::Runtime {
            provider: &session.provider,
            account_id: account.as_deref(),
        }),
    );
}

pub fn ingest_statusline_quota(
    svc: &ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    body: &Value,
) -> bool {
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    ingest_statusline_quota_at(svc, tab_id, session_id, nonce, body, now_ms)
}

pub(crate) fn ingest_statusline_quota_at(
    svc: &ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    body: &Value,
    now_ms: u64,
) -> bool {
    if session_id.is_empty() || nonce.is_empty() {
        return false;
    }
    let (provider, account) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return false;
        };
        if tab.agent_session_id.as_deref() != Some(session_id)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
        {
            return false;
        }
        let Some(provider) = tab.agent_type.clone() else {
            return false;
        };
        let account = launch_profile::of_tab(tab)
            .map(|profile| limits_source::balance_account_key(&profile))
            .or_else(|| {
                tab.restorable_launch
                    .account_id
                    .clone()
                    .filter(|id| id != codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT)
            });
        (provider, account)
    };
    if !crate::config::usage_tracking_enabled_from(&svc.configuration(), &provider) {
        return false;
    }
    let Some(quota) = parse_statusline_quota(body, now_ms) else {
        return false;
    };
    let has_limits = quota
        .get("limits")
        .and_then(Value::as_array)
        .is_some_and(|limits| !limits.is_empty());
    if !has_limits {
        return false;
    }
    let sample = codeterm_usage::UsageSample {
        usage_details: Some(json!({
            "limits": [],
            "dollar_credits": [],
            "extra_usage": null,
            "quota": quota,
        })),
        provider: provider.clone(),
        billing_mode: BillingMode::Subscription,
        limits_reported: true,
        account_id: account.clone(),
        captured_at_ms: now_ms as i64,
        session_pct: None,
        session_resets_at_ms: None,
        weekly_pct: None,
        weekly_resets_at_ms: None,
        spend_cents: None,
        spend_limit_cents: None,
        spend_remaining_cents: None,
        spend_resets_at_ms: None,
        raw_text: String::new(),
    };
    let Ok(snapshot_id) = svc.usage_store.insert_runtime_snapshot(&sample) else {
        return false;
    };
    crate::usage::projection::invalidate_cache(&provider, account.as_deref());
    clear_quota_reset(&provider, account.as_deref());
    svc.notify_event(
        "usage-snapshot-updated",
        json!(UsageSnapshotEvent::Snapshot {
            provider: &provider,
            account_id: account.as_deref(),
            snapshot_id,
        }),
    );
    true
}

#[cfg(test)]
mod tests;
