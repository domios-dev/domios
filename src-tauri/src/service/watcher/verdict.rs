//! Tolerant watcher verdict parsing and host-side action guardrails.

use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

use crate::multiplexer::types::WatcherPolicy;

pub const STATE_BLOB_MAX_BYTES: usize = 16 * 1024;

const MALFORMED_SUMMARY_MAX: usize = 500;

static FENCE_JSON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)```(?:json)?\s*\r?\n?(.*?)```").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatcherVerdictStatus {
    Ok,
    Attention,
    Stalled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatcherAction {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatcherVerdict {
    pub status: WatcherVerdictStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<WatcherAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerdictOutcome {
    pub verdict: WatcherVerdict,
    pub allowed_actions: Vec<WatcherAction>,
    pub dropped: Vec<(WatcherAction, &'static str)>,
    pub state: Value,
    pub state_rejected: bool,
}

pub fn parse_verdict(raw: &str) -> WatcherVerdict {
    let Some(value) = extract_json_object(raw) else {
        return WatcherVerdict {
            status: WatcherVerdictStatus::Attention,
            summary: raw_prefix(raw, MALFORMED_SUMMARY_MAX),
            state: None,
            actions: vec![],
        };
    };
    parse_verdict_value(&value, raw)
}

fn parse_verdict_value(value: &Value, raw_fallback: &str) -> WatcherVerdict {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .and_then(parse_status);
    let Some(status) = status else {
        return WatcherVerdict {
            status: WatcherVerdictStatus::Attention,
            summary: value
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| raw_prefix(raw_fallback, MALFORMED_SUMMARY_MAX)),
            state: value.get("state").cloned(),
            actions: parse_actions(value.get("actions")),
        };
    };

    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| raw_prefix(raw_fallback, MALFORMED_SUMMARY_MAX));

    WatcherVerdict {
        status,
        summary,
        state: value.get("state").cloned(),
        actions: parse_actions(value.get("actions")),
    }
}

fn parse_status(s: &str) -> Option<WatcherVerdictStatus> {
    match s.to_ascii_lowercase().as_str() {
        "ok" => Some(WatcherVerdictStatus::Ok),
        "attention" => Some(WatcherVerdictStatus::Attention),
        "stalled" => Some(WatcherVerdictStatus::Stalled),
        _ => None,
    }
}

fn parse_actions(v: Option<&Value>) -> Vec<WatcherAction> {
    let Some(Value::Array(items)) = v else {
        return vec![];
    };
    items
        .iter()
        .filter_map(|item| {
            let kind = item.get("kind")?.as_str()?.to_string();
            let message = item.get("message")?.as_str()?.to_string();
            let tab = item.get("tab").and_then(Value::as_str).map(str::to_string);
            Some(WatcherAction {
                kind,
                tab,
                message,
            })
        })
        .collect()
}

fn raw_prefix(raw: &str, max: usize) -> String {
    raw.chars().take(max).collect()
}

fn extract_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Some(v);
        }
    }
    for caps in FENCE_JSON_RE.captures_iter(raw) {
        let body = caps.get(1)?.as_str().trim();
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            if v.is_object() {
                return Some(v);
            }
        }
    }
    let mut best: Option<(Value, usize)> = None;
    for (idx, ch) in raw.char_indices() {
        if ch != '{' {
            continue;
        }
        if let Some((value, end)) = parse_object_at(raw, idx) {
            let len = end - idx;
            if best.as_ref().is_none_or(|(_, best_len)| len > *best_len) {
                best = Some((value, len));
            }
        }
    }
    best.map(|(v, _)| v)
}

fn parse_object_at(text: &str, start: usize) -> Option<(Value, usize)> {
    let slice = object_slice(text, start)?;
    serde_json::from_str::<Value>(slice.0)
        .ok()
        .filter(Value::is_object)
        .map(|v| (v, slice.1))
}

fn object_slice(text: &str, start: usize) -> Option<(&str, usize)> {
    if !text[start..].starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (rel, ch) in text[start..].char_indices() {
        let idx = start + rel;
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = idx + ch.len_utf8();
                    return Some((&text[start..end], end));
                }
            }
            _ => {}
        }
    }
    None
}

fn state_blob_bytes(state: &Value) -> usize {
    serde_json::to_string(state).map(|s| s.len()).unwrap_or(0)
}

fn action_targets_group(action: &WatcherAction, group: &HashSet<String>) -> bool {
    match action.kind.as_str() {
        "notify" => true,
        "nudge" | "report" => action.tab.as_deref().is_some_and(|p| group.contains(p)),
        _ => false,
    }
}

fn nudge_on_cooldown(
    action: &WatcherAction,
    cooldowns: &HashMap<String, u64>,
    cooldown_secs: u64,
    now_ms: u64,
) -> bool {
    if action.kind != "nudge" {
        return false;
    }
    let Some(tab) = action.tab.as_deref() else {
        return false;
    };
    let Some(&last_ms) = cooldowns.get(tab) else {
        return false;
    };
    now_ms.saturating_sub(last_ms) < cooldown_secs.saturating_mul(1000)
}

pub fn apply_guardrails(
    verdict: &WatcherVerdict,
    policy: &WatcherPolicy,
    group: &HashSet<String>,
    cooldowns: &HashMap<String, u64>,
    now_ms: u64,
    previous_state: &Value,
) -> VerdictOutcome {
    let (state, state_rejected) = match verdict.state.as_ref() {
        Some(s) if state_blob_bytes(s) > STATE_BLOB_MAX_BYTES => (previous_state.clone(), true),
        Some(s) => (s.clone(), false),
        None => (previous_state.clone(), false),
    };

    let mut allowed = Vec::new();
    let mut dropped = Vec::new();
    let mut nudged_this_tick: HashSet<String> = HashSet::new();

    for action in &verdict.actions {
        if !matches!(action.kind.as_str(), "nudge" | "notify" | "report") {
            dropped.push((action.clone(), "out_of_group"));
            continue;
        }
        if !action_targets_group(action, group) {
            dropped.push((action.clone(), "out_of_group"));
            continue;
        }
        if action.kind == "nudge" {
            if let Some(tab) = action.tab.as_deref() {
                if nudge_on_cooldown(action, cooldowns, policy.nudge_cooldown_secs, now_ms) {
                    dropped.push((action.clone(), "cooldown"));
                    continue;
                }
                if !nudged_this_tick.insert(tab.to_string()) {
                    dropped.push((action.clone(), "cooldown"));
                    continue;
                }
            }
        }
        allowed.push(action.clone());
    }

    let max = policy.max_actions_per_tick as usize;
    if allowed.len() > max {
        let overflow = allowed.split_off(max);
        for action in overflow {
            dropped.push((action, "overflow"));
        }
    }

    let mut normalized = verdict.clone();
    normalized.state = Some(state.clone());

    VerdictOutcome {
        verdict: normalized,
        allowed_actions: allowed,
        dropped,
        state,
        state_rejected,
    }
}

#[cfg(test)]
mod tests;
