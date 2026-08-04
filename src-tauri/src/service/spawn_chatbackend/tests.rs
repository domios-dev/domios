use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use super::{
    accept_watcher_verdict_id, apply_watcher_outcome_to_spec, clear_stale_watcher_inflight_spec,
    delta_signature, execute_watcher_actions_with, extract_watcher_verdicts, next_poll_delay_ms,
    plan_poll_tick, plan_watcher_tick, tick_input_json, WatcherTickPlan, POLL_ACTIVE_MS,
    POLL_IDLE_MS,
};
use crate::agent::chat_types::{ChatMsg, StructuredChat};
use crate::multiplexer::types::{WatcherPolicy, WatcherSpec};
use crate::service::watcher::{apply_guardrails, parse_verdict, WatcherVerdictStatus};

fn delta(n: usize, cursor: Option<&str>) -> StructuredChat {
    StructuredChat {
        messages: (0..n)
            .map(|i| ChatMsg {
                id: i.to_string(),
                ..Default::default()
            })
            .collect(),
        cursor: cursor.map(|c| c.to_string()),
        done: None,
    }
}

#[test]
fn cadence_is_fast_while_active() {
    assert_eq!(next_poll_delay_ms(true, POLL_IDLE_MS), POLL_ACTIVE_MS);
    assert_eq!(next_poll_delay_ms(true, POLL_ACTIVE_MS), POLL_ACTIVE_MS);
    const { assert!(POLL_ACTIVE_MS <= 80, "active cadence must be near-token") };
}

#[test]
fn cadence_backs_off_to_idle_when_quiet() {
    let mut d = POLL_ACTIVE_MS;
    let mut steps = 0;
    loop {
        let next = next_poll_delay_ms(false, d);
        assert!((POLL_ACTIVE_MS..=POLL_IDLE_MS).contains(&next));
        assert!(next > d || next == POLL_IDLE_MS, "must grow until capped");
        d = next;
        steps += 1;
        if d == POLL_IDLE_MS {
            break;
        }
        assert!(steps < 20, "must converge to idle cap");
    }
    assert_eq!(next_poll_delay_ms(false, POLL_IDLE_MS), POLL_IDLE_MS);
}

#[test]
fn tick_emits_and_advances_cursor_on_nonempty_delta() {
    let tick = plan_poll_tick(&delta(2, Some("c2")), Some("c1".into()), POLL_IDLE_MS, None);
    assert!(tick.emit, "non-empty delta must emit");
    assert_eq!(tick.cursor.as_deref(), Some("c2"), "cursor advances");
    assert_eq!(tick.delay_ms, POLL_ACTIVE_MS, "active => fast next poll");
}

#[test]
fn tick_idle_does_not_emit_and_keeps_cursor() {
    let tick = plan_poll_tick(&delta(0, None), Some("c1".into()), POLL_ACTIVE_MS, None);
    assert!(!tick.emit, "empty delta must NOT emit");
    assert_eq!(
        tick.cursor.as_deref(),
        Some("c1"),
        "cursor unchanged when idle"
    );
    assert!(tick.delay_ms > POLL_ACTIVE_MS, "idle backs off");
}

#[test]
fn tick_first_poll_no_cursor_emits_full_transcript() {
    let tick = plan_poll_tick(&delta(3, Some("c0")), None, POLL_IDLE_MS, None);
    assert!(tick.emit);
    assert_eq!(tick.cursor.as_deref(), Some("c0"));
}

#[test]
fn tick_suppresses_byte_identical_delta() {
    let d = delta(1, Some("liveFrom"));
    let sig = delta_signature(&d);
    let t1 = plan_poll_tick(&d, Some("c0".into()), POLL_ACTIVE_MS, None);
    assert!(t1.emit, "first delta must emit");
    assert_eq!(t1.signature, Some(sig));
    let t2 = plan_poll_tick(&d, t1.cursor.clone(), t1.delay_ms, t1.signature);
    assert!(!t2.emit, "byte-identical re-returned delta must NOT emit");
    assert_eq!(t2.signature, Some(sig), "signature unchanged");
    assert_eq!(t2.delay_ms, POLL_ACTIVE_MS, "still active while streaming");
}

#[test]
fn tick_emits_when_delta_grows() {
    let d1 = delta(1, Some("c1"));
    let t1 = plan_poll_tick(&d1, None, POLL_IDLE_MS, None);
    assert!(t1.emit);
    let d2 = delta(2, Some("c2"));
    let t2 = plan_poll_tick(&d2, t1.cursor.clone(), t1.delay_ms, t1.signature);
    assert!(t2.emit, "grown delta must emit");
    assert_ne!(t2.signature, t1.signature, "signature reflects new content");
}

#[test]
fn tick_idle_preserves_last_signature_for_later_dedup() {
    let d = delta(1, Some("c1"));
    let t1 = plan_poll_tick(&d, None, POLL_ACTIVE_MS, None);
    assert!(t1.emit);
    let idle = plan_poll_tick(
        &delta(0, None),
        t1.cursor.clone(),
        t1.delay_ms,
        t1.signature,
    );
    assert!(!idle.emit, "empty delta does not emit");
    assert_eq!(idle.signature, t1.signature, "idle keeps prior signature");
    let again = plan_poll_tick(&d, idle.cursor.clone(), idle.delay_ms, idle.signature);
    assert!(
        !again.emit,
        "same content after an idle gap must stay suppressed"
    );
}

#[test]
fn poll_loop_spawn_does_not_need_ambient_runtime() {
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "test must reproduce the no-ambient-runtime context"
    );

    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let bare = std::panic::catch_unwind(|| {
        tokio::spawn(async {});
    });
    std::panic::set_hook(prev);
    assert!(
        bare.is_err(),
        "precondition: tokio::spawn must panic without an ambient runtime"
    );

    let (tx, rx) = mpsc::channel();
    super::spawn_bg(async move {
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("poll loop future must run on the tauri runtime, not the ambient one");
}

fn watcher_spec() -> WatcherSpec {
    WatcherSpec {
        charter: "watch".into(),
        policy: WatcherPolicy::clamped(Some(30), Some(60), Some(3)),
        observations: json!({ "chatTail": 2 }),
        state: json!({ "phase": "start" }),
        tick: 4,
        last_tick_ms: Some(1_000),
        last_status: None,
        tick_inflight_since_ms: None,
    }
}

#[test]
fn watcher_due_tab_fires_exactly_one_tick_per_interval() {
    let mut spec = watcher_spec();
    let mut watcher_tick_calls = 0;

    let plan = plan_watcher_tick(&spec, 31_000, false);
    assert_eq!(plan, WatcherTickPlan::Arm { tick: 5 });
    let WatcherTickPlan::Arm { tick } = plan else {
        unreachable!();
    };
    watcher_tick_calls += 1;
    spec.tick = tick;
    spec.tick_inflight_since_ms = Some(31_000);
    let input = tick_input_json(tick, 31_000, spec.state.clone(), json!({ "tabs": [] }));
    assert_eq!(input["tick"], 5);
    assert_eq!(input["nowMs"], 31_000);
    assert_eq!(input["state"], json!({ "phase": "start" }));
    assert!(input.get("observations").is_some());

    assert_eq!(
        plan_watcher_tick(&spec, 31_001, false),
        WatcherTickPlan::BlockedInflight
    );
    assert_eq!(watcher_tick_calls, 1);
}

#[test]
fn watcher_does_not_tick_while_plugin_explicitly_busy_or_inflight() {
    let mut spec = watcher_spec();
    assert_eq!(
        plan_watcher_tick(&spec, 31_000, true),
        WatcherTickPlan::Wait
    );

    spec.tick_inflight_since_ms = Some(30_000);
    assert_eq!(
        plan_watcher_tick(&spec, 90_000, true),
        WatcherTickPlan::BlockedInflight
    );
}

#[test]
fn watcher_verdict_apply_path_clears_inflight_updates_state_and_yields_actions() {
    let delta = StructuredChat {
        messages: vec![ChatMsg {
            id: "v1".into(),
            kind: "watcher_verdict".into(),
            content: r#"{"status":"stalled","summary":"needs help","state":{"phase":"stalled"},"actions":[{"kind":"nudge","tab":"worker-1","message":"status?"}]}"#.into(),
            ..Default::default()
        }],
        cursor: Some("c1".into()),
        done: Some(true),
    };
    let verdicts = extract_watcher_verdicts(&delta);
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0].0, "v1");

    let verdict = parse_verdict(&verdicts[0].1);
    assert_eq!(verdict.status, WatcherVerdictStatus::Stalled);
    let group = HashSet::from(["worker-1".to_string()]);
    let outcome = apply_guardrails(
        &verdict,
        &WatcherPolicy::clamped(Some(30), Some(60), Some(3)),
        &group,
        &HashMap::new(),
        42_000,
        &json!({ "phase": "old" }),
    );
    assert_eq!(outcome.state, json!({ "phase": "stalled" }));
    assert_eq!(outcome.allowed_actions.len(), 1);

    let mut spec = watcher_spec();
    spec.tick_inflight_since_ms = Some(31_000);
    let actions = outcome.allowed_actions.clone();
    assert!(apply_watcher_outcome_to_spec(&mut spec, outcome, 42_000));
    let mut cooldowns = HashMap::new();
    let mut nudges: Vec<(String, String)> = Vec::new();
    let mut notifications: Vec<String> = Vec::new();
    let mut reports: Vec<String> = Vec::new();
    execute_watcher_actions_with(
        "watcher-1",
        &actions,
        42_000,
        &mut cooldowns,
        |tab, message| {
            nudges.push((tab.to_string(), message.to_string()));
            Ok(())
        },
        |message| notifications.push(message.to_string()),
        |message| {
            reports.push(message.to_string());
            Ok(())
        },
    );
    assert_eq!(spec.state, json!({ "phase": "stalled" }));
    assert_eq!(spec.last_status.as_deref(), Some("stalled"));
    assert_eq!(spec.last_tick_ms, Some(42_000));
    assert_eq!(spec.tick_inflight_since_ms, None);
    assert_eq!(
        nudges,
        vec![("worker-1".to_string(), "status?".to_string())]
    );
    assert_eq!(cooldowns.get("worker-1"), Some(&42_000));
    assert!(notifications.is_empty());
    assert!(reports.is_empty());
}

#[test]
fn late_verdict_after_watchdog_clear_is_dropped() {
    let mut spec = watcher_spec();
    spec.tick_inflight_since_ms = Some(1_000);
    assert!(clear_stale_watcher_inflight_spec(&mut spec, 301_000));
    assert_eq!(spec.tick_inflight_since_ms, None);
    assert_eq!(spec.last_tick_ms, Some(301_000));

    let verdict = parse_verdict(
        r#"{"status":"stalled","summary":"late","state":{"phase":"late"},"actions":[{"kind":"notify","message":"late"}]}"#,
    );
    let outcome = apply_guardrails(
        &verdict,
        &spec.policy,
        &HashSet::new(),
        &HashMap::new(),
        302_000,
        &spec.state,
    );
    let actions = outcome.allowed_actions.clone();
    let applied = apply_watcher_outcome_to_spec(&mut spec, outcome, 302_000);
    let mut action_count = 0;
    let mut cooldowns = HashMap::new();
    if applied {
        execute_watcher_actions_with(
            "watcher-1",
            &actions,
            302_000,
            &mut cooldowns,
            |_, _| Ok(()),
            |_| action_count += 1,
            |_| Ok(()),
        );
    }
    assert!(!applied);
    assert_eq!(action_count, 0);
    assert_eq!(spec.state, json!({ "phase": "start" }));
    assert_eq!(spec.last_status, None);
}

#[test]
fn replayed_verdict_id_executes_actions_once() {
    let mut seen = HashMap::new();
    let mut action_count = 0;
    let tab_id = "watcher-1";
    let verdict_id = "verdict-1";
    let raw = r#"{"status":"attention","summary":"notify","actions":[{"kind":"notify","message":"heads up"}]}"#;

    for _ in 0..2 {
        if !accept_watcher_verdict_id(&mut seen, tab_id, verdict_id) {
            continue;
        }
        let verdict = parse_verdict(raw);
        let outcome = apply_guardrails(
            &verdict,
            &WatcherPolicy::clamped(Some(30), Some(60), Some(3)),
            &HashSet::new(),
            &HashMap::new(),
            42_000,
            &json!({}),
        );
        action_count += outcome.allowed_actions.len();
    }

    assert_eq!(action_count, 1);
}

#[test]
fn restored_tab_with_ancient_last_tick_fires_one_tick_not_burst() {
    let mut restored = watcher_spec();
    restored.last_tick_ms = Some(1_000);
    restored.tick_inflight_since_ms = None;
    restored.state = json!({ "phase": "restored" });

    assert_eq!(
        plan_watcher_tick(&restored, 301_000, false),
        WatcherTickPlan::Arm { tick: 5 }
    );
    restored.tick = 5;
    restored.tick_inflight_since_ms = Some(301_000);
    assert_eq!(
        plan_watcher_tick(&restored, 301_001, false),
        WatcherTickPlan::BlockedInflight
    );
    assert_eq!(restored.state, json!({ "phase": "restored" }));
}

#[test]
fn guardrail_group_includes_the_orchestrator() {
    let group = super::guardrail_group(crate::service::watcher::WatchGroup {
        orchestrator_id: "orch-1".into(),
        tab_ids: vec!["worker-1".into(), "planner-1".into()],
    });
    assert!(
        group.contains("orch-1"),
        "nudging the own orchestrator is the canonical case"
    );
    assert!(group.contains("worker-1"));
    assert_eq!(group.len(), 3);
}
