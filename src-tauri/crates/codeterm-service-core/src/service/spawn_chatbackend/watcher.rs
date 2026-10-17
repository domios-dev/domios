use super::*;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// Adaptive inbound-poll cadence bounds: fast while streaming, geometric
/// back-off to idle so an open-but-quiet chat tab costs almost nothing.
pub(super) const POLL_ACTIVE_MS: u64 = 60;
pub(super) const POLL_IDLE_MS: u64 = 750;

pub(super) fn next_poll_delay_ms(had_delta: bool, current_ms: u64) -> u64 {
    if had_delta {
        POLL_ACTIVE_MS
    } else {
        current_ms
            .saturating_mul(2)
            .clamp(POLL_ACTIVE_MS, POLL_IDLE_MS)
    }
}

/// Byte-identity signature of a delta's payload, so a streaming message
/// re-returned unchanged hashes the same and the redundant emit is suppressed.
pub(super) fn delta_signature(delta: &StructuredChat) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&delta.messages)
        .unwrap_or_default()
        .hash(&mut h);
    h.finish()
}

pub(super) struct PollTick {
    pub(super) cursor: Option<String>,
    pub(super) emit: bool,
    pub(super) delay_ms: u64,
    pub(super) signature: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatcherTickPlan {
    Arm { tick: u64 },
    BlockedInflight,
    Wait,
}

pub(super) fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn watcher_interval_ms(policy: &WatcherPolicy) -> u64 {
    policy.tick_interval_secs.saturating_mul(1000)
}

pub(super) fn watcher_watchdog_timeout_ms(policy: &WatcherPolicy) -> u64 {
    watcher_interval_ms(policy).saturating_mul(2).max(300_000)
}

pub(super) fn plan_watcher_tick(
    spec: &WatcherSpec,
    now_ms: u64,
    plugin_busy: bool,
) -> WatcherTickPlan {
    if spec.tick_inflight_since_ms.is_some() {
        return WatcherTickPlan::BlockedInflight;
    }
    if plugin_busy {
        return WatcherTickPlan::Wait;
    }
    let interval_ms = watcher_interval_ms(&spec.policy);
    let due = spec
        .last_tick_ms
        .map(|last| now_ms >= last.saturating_add(interval_ms))
        .unwrap_or(true);
    if due {
        WatcherTickPlan::Arm {
            tick: spec.tick.saturating_add(1),
        }
    } else {
        WatcherTickPlan::Wait
    }
}

pub(super) fn watcher_inflight_stale(spec: &WatcherSpec, now_ms: u64) -> bool {
    spec.tick_inflight_since_ms.is_some_and(|since| {
        now_ms.saturating_sub(since) >= watcher_watchdog_timeout_ms(&spec.policy)
    })
}

pub(super) fn clear_stale_watcher_inflight_spec(spec: &mut WatcherSpec, now_ms: u64) -> bool {
    if watcher_inflight_stale(spec, now_ms) {
        spec.tick_inflight_since_ms = None;
        spec.last_tick_ms = Some(now_ms);
        return true;
    }
    false
}

pub(super) fn accept_watcher_verdict_id(
    last_ids: &mut HashMap<String, String>,
    tab_id: &str,
    verdict_id: &str,
) -> bool {
    if last_ids.get(tab_id).is_some_and(|last| last == verdict_id) {
        return false;
    }
    last_ids.insert(tab_id.to_string(), verdict_id.to_string());
    true
}

pub(super) fn apply_watcher_outcome_to_spec(
    spec: &mut WatcherSpec,
    outcome: VerdictOutcome,
    now_ms: u64,
) -> bool {
    if spec.tick_inflight_since_ms.is_none() {
        return false;
    }
    spec.state = outcome.state;
    spec.last_status = Some(watcher_status_label(outcome.verdict.status).to_string());
    spec.last_tick_ms = Some(now_ms);
    spec.tick_inflight_since_ms = None;
    true
}

pub(super) fn watcher_system_note(text: impl Into<String>) -> ChatMsg {
    ChatMsg {
        id: format!("watcher-note-{}", uuid::Uuid::new_v4().simple()),
        kind: "system".to_string(),
        content: text.into(),
        system_subtype: Some("watcher".to_string()),
        ..Default::default()
    }
}

pub(super) fn watcher_status_label(status: WatcherVerdictStatus) -> &'static str {
    match status {
        WatcherVerdictStatus::Ok => "ok",
        WatcherVerdictStatus::Attention => "attention",
        WatcherVerdictStatus::Stalled => "stalled",
    }
}

pub(super) fn extract_watcher_verdicts(delta: &StructuredChat) -> Vec<(String, String)> {
    delta
        .messages
        .iter()
        .filter(|m| m.kind == "watcher_verdict")
        .map(|m| (m.id.clone(), m.content.clone()))
        .collect()
}

pub(super) fn tick_input_json(
    tick: u64,
    now_ms: u64,
    state: serde_json::Value,
    observations: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "tick": tick,
        "nowMs": now_ms,
        "state": state,
        "observations": observations,
    })
}

pub(super) fn execute_watcher_actions_with<N, O, R>(
    tab_id: &str,
    actions: &[WatcherAction],
    now_ms: u64,
    cooldowns: &mut HashMap<String, u64>,
    mut nudge: N,
    mut notify: O,
    mut report: R,
) where
    N: FnMut(&str, &str) -> Result<(), String>,
    O: FnMut(&str),
    R: FnMut(&str) -> Result<(), String>,
{
    for action in actions {
        match action.kind.as_str() {
            "nudge" => {
                if let Some(target) = action.tab.as_deref() {
                    match nudge(target, &action.message) {
                        Ok(()) => {
                            cooldowns.insert(target.to_string(), now_ms);
                        }
                        Err(e) => log::warn!(
                            "[watcher] nudge from {} to {} failed: {}",
                            tab_id,
                            target,
                            e
                        ),
                    }
                }
            }
            "notify" => notify(&action.message),
            "report" => {
                if let Err(e) = report(&action.message) {
                    log::warn!("[watcher] report from {} failed: {}", tab_id, e);
                }
            }
            _ => {}
        }
    }
}

/// Nudge targets include the orchestrator itself — the canonical health
/// watcher nudges its own orchestrator, which is not a descendant.
pub(super) fn guardrail_group(watch: crate::service::watcher::WatchGroup) -> HashSet<String> {
    let mut group: HashSet<String> = watch.tab_ids.into_iter().collect();
    group.insert(watch.orchestrator_id);
    group
}

/// Pure per-tick planning for the poll loop. Emit only when the delta is
/// non-empty AND differs from the last emitted payload — a streaming message
/// re-returned unchanged is suppressed to avoid redundant IPC + O(n²) bytes.
pub(super) fn plan_poll_tick(
    delta: &StructuredChat,
    prev_cursor: Option<String>,
    prev_delay: u64,
    prev_signature: Option<u64>,
) -> PollTick {
    let had_delta = !delta.messages.is_empty();
    let signature = if had_delta {
        Some(delta_signature(delta))
    } else {
        prev_signature
    };
    PollTick {
        cursor: delta.cursor.clone().or(prev_cursor),
        emit: had_delta && signature != prev_signature,
        delay_ms: next_poll_delay_ms(had_delta, prev_delay),
        signature,
    }
}
