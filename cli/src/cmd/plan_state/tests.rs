use super::*;
use serde_json::json;

#[test]
fn renders_compact_human_view() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Loop",
        "status": "active",
        "mode": "planning",
        "goal": {"vector": "ship it"},
        "owner": {"tab_id": "tab_o"},
        "task_counts": {"total": 4, "done": 1, "in_progress": 1, "in_review": 1, "pending": 1, "blocked": 0, "cancelled": 0},
        "percent_done": 0.25,
        "coverage": {"plan_covered_fraction": 0.5, "gaps": ["s2"]},
        "drift": {"total": 1, "by_kind": {"in_progress_tab_gone": 1}},
        "heartbeat": {"current_focus": "foundation", "active_lanes": ["loop-state"]},
        "loops": [{"id": "lp_1", "status": "running", "target": "harden", "count": 2, "budget": 5, "stop_when": "0 bugs"}],
        "open_lanes": [{"task_id": "t_p", "status": "pending", "presentation": "actionable_pending", "name": "pend"}],
        "live_agents": [{"task_id": "t_r", "tab_id": "tab_live", "name": "rev"}],
        "recent_strides": [{"kind": "decision", "text": "did a thing"}]
    });
    let out = render_state(&val);
    assert!(out.contains("Loop [pl_1] (active)"));
    assert!(out.contains("goal: ship it"));
    assert!(out.contains("25% total-task progress"));
    assert!(out.contains("coverage: 50% (1 section gap"));
    assert!(out.contains("drift: 1 (in_progress_tab_gone 1)"));
    assert!(out.contains("focus: foundation"));
    assert!(out.contains("lp_1 [running] harden: 2/5; stop_when: 0 bugs"));
    assert!(out.contains("t_p [pending; actionable pending] pend"));
    assert!(out.contains("t_r -> tab_live (rev)"));
    assert!(out.contains("[decision] did a thing"));
}

#[test]
fn renders_queued_for_qa_with_operator_label_not_bare_in_review() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Queued QA",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 1, "done": 0, "in_progress": 0, "in_review": 1, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 0.0,
        "coverage": {"plan_covered_fraction": 1.0, "gaps": []},
        "open_lanes": [{
            "task_id": "task_qawait1",
            "status": "in_review",
            "presentation": "queued_for_qa",
            "status_label": "blocked: reviewer host unavailable overnight",
            "name": "Bench dc wrapper"
        }]
    });
    let out = render_state(&val);
    assert!(
        out.contains(
            "task_qawait1 [in_review; queued for qa: blocked: reviewer host unavailable overnight] Bench dc wrapper"
        ),
        "{out}"
    );
    assert!(!out.contains("in_review; in review"), "{out}");
}

#[test]
fn renders_queued_for_qa_without_label_degrades_honestly() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Queued QA",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 1, "done": 0, "in_progress": 0, "in_review": 1, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 0.0,
        "coverage": {"plan_covered_fraction": 1.0, "gaps": []},
        "open_lanes": [{
            "task_id": "task_x",
            "status": "in_review",
            "presentation": "queued_for_qa",
            "name": "No label yet"
        }]
    });
    let out = render_state(&val);
    assert!(
        out.contains("task_x [in_review; queued for qa] No label yet"),
        "{out}"
    );
}

#[test]
fn renders_dependency_waiting_pending_as_distinct_from_paused() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Sequenced",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 2, "done": 0, "in_progress": 1, "in_review": 0, "pending": 1, "blocked": 0, "cancelled": 0},
        "percent_done": 0.0,
        "open_lanes": [
            {"task_id": "task_ready", "status": "pending", "presentation": "actionable_pending", "name": "Ready"},
            {"task_id": "task_after", "status": "pending", "presentation": "waiting_on_dependencies", "dependency_waits": [{"task_id": "task_before"}], "name": "After"}
        ]
    });
    let out = render_state(&val);
    assert!(out.contains("task_ready [pending; actionable pending] Ready"));
    assert!(out.contains("task_after [pending; waiting on dependency: task_before] After"));
    assert!(!out.contains("parked"));
}

#[test]
fn percent_done_matches_displayed_counts() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Paused work",
        "status": "active",
        "mode": "execution",
        "task_counts": {
            "total": 2,
            "done": 1,
            "in_progress": 0,
            "in_review": 0,
            "pending": 0,
            "blocked": 0,
            "paused": 1,
            "cancelled": 0
        },
        "percent_done": 1.0
    });

    let out = render_state(&val);

    assert!(out.contains("paused 1"), "{out}");
    assert!(out.contains("50% total-task progress"), "{out}");
    assert!(!out.contains("100%"), "{out}");
}

#[test]
fn renders_pending_regression_advisory_and_override() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Gated",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 2, "done": 2, "in_progress": 0, "in_review": 0, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 1.0,
        "pending_regression_gates": [{"key": "phase-1", "label": "Phase 1"}, {"key": "phase-2", "label": "Phase 2"}],
        "blocking_regression_gates": [{"key": "phase-2", "label": "Phase 2"}],
        "regression_overrides": [{"actor": {"tab_id": "tab_o"}, "reason": "hotfix under sign-off", "gates": ["phase-1"], "ts": 42}]
    });
    let out = render_state(&val);
    assert!(out.contains("regression gates pending: phase-1, phase-2"));
    assert!(out.contains("dispatch: allowed; terminal-group gate advisory: phase-2"));
    assert!(out.contains("override by tab_o @ 42 [phase-1]: hotfix under sign-off"));
}

#[test]
fn renders_waived_gate_as_a_pending_dispatch_advisory() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Cleared",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 1, "done": 1, "in_progress": 0, "in_review": 0, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 1.0,
        "pending_regression_gates": [{"key": "phase-1", "label": "Phase 1"}],
        "blocking_regression_gates": [],
        "regression_overrides": [{"actor": {"tab_id": "tab_o"}, "reason": "waiver", "gates": ["phase-1"], "ts": 7}]
    });
    let out = render_state(&val);
    assert!(out.contains("regression gates pending: phase-1"));
    assert!(out.contains("dispatch: allowed; terminal-group gate advisory: phase-1"));
}

#[test]
fn renders_pending_group_retrospective_as_a_dispatch_advisory() {
    let val = json!({
        "plan_id": "pl_retro",
        "title": "Retro",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 1, "done": 1, "in_progress": 0, "in_review": 0, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 1.0,
        "pending_regression_gates": [],
        "blocking_regression_gates": [],
        "pending_group_retrospectives": [{"key": "phase-1", "label": "Phase 1"}]
    });
    let out = render_state(&val);
    assert!(out.contains("phase-1"));
    assert!(out.contains("pl_retro"));
    assert!(out.contains("dispatch: allowed; terminal-group gate advisory: phase-1"));
}

#[test]
fn omits_regression_block_when_nothing_pending() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Clean",
        "status": "active",
        "mode": "execution",
        "task_counts": {"total": 1, "done": 0, "in_progress": 1, "in_review": 0, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 0.0
    });
    let out = render_state(&val);
    assert!(!out.contains("regression gates"));
    assert!(!out.contains("dispatch:"));
}

#[test]
fn renders_planning_mode_without_task_alarm() {
    let val = json!({
        "plan_id": "pl_1",
        "title": "Planning",
        "status": "active",
        "mode": "planning",
        "task_counts": {"total": 0, "done": 0, "in_progress": 0, "in_review": 0, "pending": 0, "blocked": 0, "cancelled": 0},
        "percent_done": 0.0,
        "coverage": {"plan_covered_fraction": 0.0, "gaps": ["Execution"]},
        "planning_readiness": {"has_goal": true, "has_spec_source": true, "has_plan_sections": true, "spec_link_coverage": 1.0, "linked_plan_sections": 1, "total_plan_sections": 1, "has_approval_gate_hint": true, "has_verification_hint": true}
    });
    let out = render_state(&val);
    assert!(out.contains("mode: planning"));
    assert!(out.contains("planning: delegate structure drafting to the Planner"));
    assert!(out.contains("coverage note: this is task coverage"));
    assert!(out.contains("planning readiness: goal=true"));
}

#[test]
fn state_rendering_exposes_window_totals_and_expansion_hint() {
    let value = json!({"plan_id":"summary","details":{"windows":{"open_lanes":{"total":1162,"returned":50,"hint":"1112 more; use --page 2 or --all"}}}});
    let output = render_state(&value);
    assert!(output.contains("hint: open_lanes: 1112 more; use --page 2 or --all"));
}
