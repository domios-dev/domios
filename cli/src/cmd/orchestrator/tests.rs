use serde_json::json;

#[test]
fn status_renders_null_and_missing_reminders_without_debug_values() {
    let compact = compact_status(&json!({
        "live_lanes": {"open":[{"task_id":"opaque-31"}], "agents":[]},
        "reminder":{"last_tick":null, "last_due":83, "delivery_failure_retry_reason":"fragment-α"}
    }));
    let rendered = render_status(&compact);
    assert!(rendered.contains("live_lanes: open=0 blocked=0 agents=0\n"));
    assert!(rendered.contains("tasks: open=1 blocked_or_gated=0\n"));
    assert!(rendered
        .contains("reminder: last_tick=- last_due=83 last_delivery=- retry_reason=fragment-α\n"));
}

use super::{
    action_command, compact_status, render_reseed_tab_lookup_error,
    render_reseed_tab_session_error, render_status,
};
use crate::client::{RpcError, SocketCallError};

#[test]
fn status_renderer_includes_exact_commands() {
    let rendered = render_status(&compact_status(&json!({
        "plan_id": "pl_1",
        "tab_id": "tab1",
        "live_lanes": {
            "open": [{"task_id": "task_a"}],
            "blocked": [],
            "agents": [{"task_id": "task_a", "tab_id": "worker1"}],
            "heartbeat": {"active_lanes": ["L2"]}
        },
        "drift": {"total": 2, "items": []},
        "next_actions": [
            {"kind": "gate", "task_id": "task_a", "message": "gate task_a"},
            {"kind": "reconcile", "message": "reconcile"}
        ]
    })));

    assert!(rendered.contains("live_lanes: open=1 blocked=0 agents=1"));
    assert!(rendered.contains("drift: 2"));
    assert!(rendered.contains("domios task show task_a && domios plan task close pl_1 task_a"));
    assert!(rendered.contains("domios plan reconcile pl_1"));
}

#[test]
fn status_renderer_counts_dependency_gated_lanes_as_blocked() {
    let rendered = render_status(&compact_status(&json!({
        "plan_id": "pl_1",
        "tab_id": "tab1",
        "live_lanes": {
            "open": [
                {"task_id": "task_audit"},
                {"task_id": "task_a"}, {"task_id": "task_b"}, {"task_id": "task_c"},
                {"task_id": "task_d"}, {"task_id": "task_e"}, {"task_id": "task_f"}
            ],
            "blocked": [],
            "agents": [{"task_id": "task_a", "tab_id": "worker1"}]
        },
        "blocked_or_gated_count": 6,
        "drift": {"total": 0, "items": []},
        "next_actions": []
    })));

    assert!(rendered.contains("live_lanes: open=1 blocked=0 agents=1"));
    assert!(rendered.contains("tasks: open=7 blocked_or_gated=6"));
}

#[test]
fn compact_status_omits_lane_bodies() {
    let compact = compact_status(&json!({
        "plan_id": "pl_1",
        "tab_id": "tab1",
        "live_lanes": {
            "open": [{"task_id": "task_a", "large": "payload"}],
            "blocked": [],
            "agents": [{"task_id": "task_a", "tab_id": "worker1", "transcript": "large"}]
        },
        "drift": {"items": [{"reason": "drift body"}]},
        "next_actions": [{"kind": "dispatch", "task_id": "task_a", "message": "very long action body"}],
        "reminder": {"last_tick": 1}
    }));

    assert_eq!(compact["live_lanes"]["open"], 1);
    assert_eq!(compact["live_lanes"]["agents"], 1);
    assert_eq!(compact["drift"], 1);
    assert_eq!(compact["next_actions_total"], 1);
    let encoded = compact.to_string();
    assert!(!encoded.contains("transcript"));
    assert!(!encoded.contains("drift body"));
    assert!(!encoded.contains("very long action body"));
}

#[test]
fn compact_status_keeps_the_parked_group_count() {
    let compact = compact_status(&json!({
        "plan_id": "pl_1",
        "next_actions": [{
            "kind": "parked_for_later_groups",
            "task_id": null,
            "message": "3 tasks parked for later groups"
        }]
    }));
    assert_eq!(
        compact["next_actions"][0]["message"],
        "3 tasks parked for later groups"
    );
    assert!(render_status(&compact).contains("3 tasks parked for later groups"));
}

#[test]
fn orchestrator_status_surfaces_report_delivery_drift() {
    let rendered = render_status(&compact_status(&json!({
        "plan_id": "pl_1",
        "tab_id": "orch",
        "live_lanes": {"open": [], "blocked": [], "agents": []},
        "drift": {
            "total": 1,
            "items": [{
                "task_id": "task_a",
                "kind": "report_target_write_missing",
                "detail": "in_review report for attempt attempt_1 was not written to target tab orch after 30001 ms",
                "suggested_fix": "deliver the report to tab orch"
            }]
        },
        "next_actions": []
    })));

    assert!(rendered.contains("task task_a"));
    assert!(rendered.contains("attempt_1"));
    assert!(rendered.contains("target tab orch"));
}

#[test]
fn action_commands_cover_task_and_plan_context() {
    assert_eq!(
        action_command("pl_1", &json!({"kind": "dispatch", "task_id": "task_b"})),
        "domios task show task_b"
    );
    assert_eq!(
        action_command("pl_1", &json!({"kind": "reconcile"})),
        "domios plan reconcile pl_1"
    );
}

#[test]
fn reseed_tab_lookup_error_includes_agent_recovery_command() {
    let msg = render_reseed_tab_lookup_error(
        "tab1",
        SocketCallError::Rpc(RpcError {
            code: -32603,
            message: "Tab not found".into(),
            data: None,
        }),
    );

    assert!(msg.contains("cannot inspect tab tab1 for handoff reseed"));
    assert!(msg.contains("domios tab list --agents"));
    assert!(msg.contains("--tab <tab-id>"));
    assert!(!msg.contains("RPC error"));
    assert!(!msg.contains("-32603"));
}

#[test]
fn reseed_session_error_includes_agent_recovery_command() {
    let msg = render_reseed_tab_session_error("tab1");

    assert!(msg.contains("cannot reseed because tab tab1 is not an agent or has no session"));
    assert!(msg.contains("domios tab list --agents"));
    assert!(msg.contains("--tab <tab-id>"));
}

#[test]
fn compact_status_counts_the_whole_plan_when_detail_lists_are_windowed() {
    let value = json!({
        "plan_id":"windowed", "live_lanes":{"open":[{"task_id":"shown"}],"blocked":[],"agents":[]},
        "next_actions":[{"kind":"dispatch","task_id":"shown"}],
        "details":{"totals":{"open_lanes":1162,"live_agents":80,"next_actions":1200},"live_open_lanes":70,"live_blocked_lanes":10},
        "drift":{"total":90,"by_kind":{"report_target_write_missing":60},"items":[]}
    });
    let compact = compact_status(&value);
    assert_eq!(compact["open_tasks"], 1162);
    assert_eq!(compact["live_lanes"]["open"], 70);
    assert_eq!(compact["live_lanes"]["blocked"], 10);
    assert_eq!(compact["live_lanes"]["agents"], 80);
    assert_eq!(compact["next_actions_total"], 1200);
    assert_eq!(compact["report_delivery_drift_total"], 60);
}
