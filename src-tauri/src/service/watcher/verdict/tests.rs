use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::multiplexer::types::WatcherPolicy;

use super::{
    apply_guardrails, parse_verdict, WatcherAction, WatcherVerdict, WatcherVerdictStatus,
    STATE_BLOB_MAX_BYTES,
};

fn group(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| (*s).to_string()).collect()
}

fn nudge(tab: &str, msg: &str) -> WatcherAction {
    WatcherAction {
        kind: "nudge".into(),
        tab: Some(tab.into()),
        message: msg.into(),
    }
}

#[test]
fn parse_naked_json() {
    let raw = r#"{"status":"ok","summary":"all good","actions":[]}"#;
    let v = parse_verdict(raw);
    assert_eq!(v.status, WatcherVerdictStatus::Ok);
    assert_eq!(v.summary, "all good");
    assert!(v.actions.is_empty());
}

#[test]
fn parse_fenced_json_block() {
    let raw = "Here is my verdict:\n```json\n{\"status\":\"stalled\",\"summary\":\"stuck\",\"actions\":[]}\n```\nThanks.";
    let v = parse_verdict(raw);
    assert_eq!(v.status, WatcherVerdictStatus::Stalled);
    assert_eq!(v.summary, "stuck");
}

#[test]
fn parse_json_with_surrounding_prose() {
    let raw = "Assessment follows. {\"status\":\"attention\",\"summary\":\"check workers\",\"actions\":[]} End.";
    let v = parse_verdict(raw);
    assert_eq!(v.status, WatcherVerdictStatus::Attention);
    assert_eq!(v.summary, "check workers");
}

#[test]
fn malformed_degrades_to_attention_without_actions() {
    let raw = "This is not JSON at all, just prose.";
    let v = parse_verdict(raw);
    assert_eq!(v.status, WatcherVerdictStatus::Attention);
    assert_eq!(v.summary, raw);
    assert!(v.actions.is_empty());
    assert!(v.state.is_none());
}

#[test]
fn unknown_status_degrades_to_attention() {
    let raw = r#"{"status":"exploded","summary":"weird","actions":[{"kind":"nudge","tab":"p1","message":"hi"}]}"#;
    let v = parse_verdict(raw);
    assert_eq!(v.status, WatcherVerdictStatus::Attention);
    assert_eq!(v.summary, "weird");
    assert_eq!(v.actions.len(), 1);
}

#[test]
fn guardrail_drops_out_of_group_nudge() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Ok,
        summary: "nudge".into(),
        state: None,
        actions: vec![nudge("outside", "go")],
    };
    let policy = WatcherPolicy::clamped(None, None, None);
    let group = group(&["inside"]);
    let out = apply_guardrails(
        &verdict,
        &policy,
        &group,
        &HashMap::new(),
        1_000,
        &json!({}),
    );
    assert!(out.allowed_actions.is_empty());
    assert_eq!(out.dropped.len(), 1);
    assert_eq!(out.dropped[0].1, "out_of_group");
}

#[test]
fn guardrail_overflow_truncates_excess_actions() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Ok,
        summary: "many".into(),
        state: None,
        actions: vec![
            nudge("p1", "a"),
            nudge("p2", "b"),
            nudge("p3", "c"),
            nudge("p4", "d"),
        ],
    };
    let policy = WatcherPolicy::clamped(None, None, Some(2));
    let group = group(&["p1", "p2", "p3", "p4"]);
    let out = apply_guardrails(&verdict, &policy, &group, &HashMap::new(), 0, &json!({}));
    assert_eq!(out.allowed_actions.len(), 2);
    assert_eq!(out.allowed_actions[0].tab.as_deref(), Some("p1"));
    assert_eq!(out.allowed_actions[1].tab.as_deref(), Some("p2"));
    assert!(out.dropped.iter().any(|(_, r)| *r == "overflow"));
}

#[test]
fn guardrail_cooldown_suppresses_recent_nudge() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Stalled,
        summary: "again".into(),
        state: None,
        actions: vec![nudge("p1", "wake")],
    };
    let policy = WatcherPolicy::clamped(None, Some(600), None);
    let group = group(&["p1"]);
    let cooldowns = HashMap::from([("p1".to_string(), 5_000u64)]);
    let out = apply_guardrails(&verdict, &policy, &group, &cooldowns, 10_000, &json!({}));
    assert!(out.allowed_actions.is_empty());
    assert_eq!(out.dropped[0].1, "cooldown");
}

#[test]
fn guardrail_dedupes_duplicate_nudge_targets_within_tick() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Stalled,
        summary: "dup".into(),
        state: None,
        actions: vec![nudge("p1", "first"), nudge("p1", "second")],
    };
    let policy = WatcherPolicy::clamped(None, None, None);
    let group = group(&["p1"]);
    let out = apply_guardrails(&verdict, &policy, &group, &HashMap::new(), 0, &json!({}));
    assert_eq!(out.allowed_actions.len(), 1);
    assert_eq!(out.allowed_actions[0].message, "first");
    assert_eq!(out.dropped.len(), 1);
    assert_eq!(out.dropped[0].1, "cooldown");
    assert_eq!(out.dropped[0].0.message, "second");
}

#[test]
fn guardrail_allows_nudges_to_distinct_tabs() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Ok,
        summary: "two".into(),
        state: None,
        actions: vec![nudge("p1", "a"), nudge("p2", "b")],
    };
    let policy = WatcherPolicy::clamped(None, None, None);
    let group = group(&["p1", "p2"]);
    let out = apply_guardrails(&verdict, &policy, &group, &HashMap::new(), 0, &json!({}));
    assert_eq!(out.allowed_actions.len(), 2);
    assert!(out.dropped.is_empty());
}

#[test]
fn guardrail_dedupes_before_overflow_cap() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Ok,
        summary: "mix".into(),
        state: None,
        actions: vec![
            nudge("p1", "a"),
            nudge("p1", "dup"),
            nudge("p2", "b"),
            nudge("p3", "c"),
        ],
    };
    let policy = WatcherPolicy::clamped(None, None, Some(2));
    let group = group(&["p1", "p2", "p3"]);
    let out = apply_guardrails(&verdict, &policy, &group, &HashMap::new(), 0, &json!({}));
    assert_eq!(out.allowed_actions.len(), 2);
    assert_eq!(out.allowed_actions[0].tab.as_deref(), Some("p1"));
    assert_eq!(out.allowed_actions[1].tab.as_deref(), Some("p2"));
    assert!(out
        .dropped
        .iter()
        .any(|(a, r)| a.message == "dup" && *r == "cooldown"));
    assert!(out
        .dropped
        .iter()
        .any(|(a, r)| a.tab.as_deref() == Some("p3") && *r == "overflow"));
}

#[test]
fn guardrail_rejects_oversized_state() {
    let big = "x".repeat(STATE_BLOB_MAX_BYTES + 1);
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Ok,
        summary: "state".into(),
        state: Some(json!({ "blob": big })),
        actions: vec![],
    };
    let previous = json!({ "keep": true });
    let out = apply_guardrails(
        &verdict,
        &WatcherPolicy::clamped(None, None, None),
        &group(&[]),
        &HashMap::new(),
        0,
        &previous,
    );
    assert!(out.state_rejected);
    assert_eq!(out.state, previous);
}

#[test]
fn guardrail_accepts_state_within_cap() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Ok,
        summary: "state".into(),
        state: Some(json!({ "tick": 2 })),
        actions: vec![],
    };
    let previous = json!({ "tick": 1 });
    let out = apply_guardrails(
        &verdict,
        &WatcherPolicy::clamped(None, None, None),
        &group(&[]),
        &HashMap::new(),
        0,
        &previous,
    );
    assert!(!out.state_rejected);
    assert_eq!(out.state, json!({ "tick": 2 }));
}

#[test]
fn notify_action_allowed_without_tab() {
    let verdict = WatcherVerdict {
        status: WatcherVerdictStatus::Attention,
        summary: "ping".into(),
        state: None,
        actions: vec![WatcherAction {
            kind: "notify".into(),
            tab: None,
            message: "heads up".into(),
        }],
    };
    let out = apply_guardrails(
        &verdict,
        &WatcherPolicy::clamped(None, None, None),
        &group(&[]),
        &HashMap::new(),
        0,
        &Value::Null,
    );
    assert_eq!(out.allowed_actions.len(), 1);
}
