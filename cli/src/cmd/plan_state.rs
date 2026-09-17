use serde_json::Value;

fn s<'a>(val: &'a Value, key: &str) -> &'a str {
    val.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn u(val: &Value, key: &str) -> u64 {
    val.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

fn gate_keys(val: &Value, field: &str) -> Vec<String> {
    val.get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.get("key").and_then(|k| k.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// Regression gates block: what still needs a real QA run, what actually blocks
// dispatch right now, and any authorized waivers standing between the two.
fn render_regression_gates(val: &Value, out: &mut String) {
    let pending = gate_keys(val, "pending_regression_gates");
    let blocking = gate_keys(val, "blocking_regression_gates");
    let retrospectives = gate_keys(val, "pending_group_retrospectives");
    let overrides = val
        .get("regression_overrides")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if pending.is_empty()
        && blocking.is_empty()
        && retrospectives.is_empty()
        && overrides.is_empty()
    {
        return;
    }
    if !pending.is_empty() {
        out.push_str(&format!(
            "regression gates pending: {}\n",
            pending.join(", ")
        ));
    }
    if !retrospectives.is_empty() {
        out.push_str(&format!(
            "group retrospectives pending: {} — record each with `codeterm plan groups close {} <group> --note \"<retrospective>\"`\n",
            retrospectives.join(", "),
            s(val, "plan_id")
        ));
    }
    if !blocking.is_empty() {
        out.push_str(&format!(
            "dispatch: BLOCKED by regression gate(s): {} — non-blocker tasks cannot dispatch until cleared or overridden\n",
            blocking.join(", ")
        ));
    } else if !retrospectives.is_empty() {
        out.push_str(&format!(
            "dispatch: BLOCKED until group retrospective(s) are recorded: {}\n",
            retrospectives.join(", ")
        ));
    } else {
        out.push_str("dispatch: unblocked (no pending regression gate blocks the frontier)\n");
    }
    for o in &overrides {
        let actor = o
            .get("actor")
            .and_then(|a| a.get("tab_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let reason = o.get("reason").and_then(|v| v.as_str()).unwrap_or("");
        let ts = o.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
        let gates = o
            .get("gates")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|g| g.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "  override by {actor} @ {ts} [{gates}]: {reason}\n"
        ));
    }
}

pub fn render_state(val: &Value) -> String {
    let mut out = String::new();

    let title = s(val, "title");
    let id = s(val, "plan_id");
    let status = s(val, "status");
    out.push_str(&format!("{title} [{id}] ({status})\n"));
    let mode = s(val, "mode");
    if !mode.is_empty() {
        out.push_str(&format!("mode: {mode}\n"));
    }
    let planning_state = s(val, "planning_state");
    if !planning_state.is_empty() {
        let unlocked = val
            .get("execution_tasks_unlocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let gate = if unlocked {
            "execution tasks unlocked"
        } else {
            "execution tasks locked until approved"
        };
        out.push_str(&format!("planning state: {planning_state} ({gate})\n"));
        if let Some(blockers) = val
            .get("planning_approval_blockers")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
        {
            let list = blockers
                .iter()
                .filter_map(|b| b.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            out.push_str(&format!("approval blockers: {list}\n"));
        }
    }
    let canonical_path = s(val, "canonical_project_path");
    if !canonical_path.is_empty() {
        out.push_str(&format!("canonical project: {canonical_path}\n"));
    }

    if let Some(vector) = val
        .get("goal")
        .and_then(|g| g.get("vector"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.push_str(&format!("goal: {vector}\n"));
    }

    let owner = val
        .get("owner")
        .and_then(|o| o.get("tab_id"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty());
    if let Some(owner) = owner {
        out.push_str(&format!("owner: {owner}\n"));
    }
    if let Some(res) = val.get("planner_resolution") {
        let kind = res.get("kind").and_then(|v| v.as_str()).unwrap_or("unset");
        match kind {
            "live" => {
                let tab = res.get("tab_id").and_then(|v| v.as_str()).unwrap_or("?");
                out.push_str(&format!("planner tab: {tab} (live)\n"));
            }
            "fallback_orchestrator" => {
                let bound = res
                    .get("bound_tab_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let reason = res.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!(
                    "planner: {bound} (fallback to orchestrator — {reason})\n"
                ));
            }
            "waived" => {
                let note = res.get("note").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("planner: waived ({note})\n"));
            }
            _ => {}
        }
    }

    if let Some(c) = val.get("task_counts") {
        let total = u(c, "total");
        let done = u(c, "done");
        let mut pct = if total == 0 {
            0.0
        } else {
            done as f64 / total as f64 * 100.0
        };
        if mode != "complete" {
            pct = pct.min(99.0);
        }
        let paused = u(c, "paused");
        let paused_text = if paused > 0 {
            format!(" | paused {paused}")
        } else {
            String::new()
        };
        out.push_str(&format!(
            "tasks: {} total | done {} | in_progress {} | in_review {} | pending {} | blocked {}{} | cancelled {} ({pct:.0}% total-task progress)\n",
            total,
            done,
            u(c, "in_progress"),
            u(c, "in_review"),
            u(c, "pending"),
            u(c, "blocked"),
            paused_text,
            u(c, "cancelled"),
        ));
        let unlocked = val
            .get("execution_tasks_unlocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(mode != "planning");
        if !unlocked {
            out.push_str(
                "planning: delegate structure drafting to the Planner; run `codeterm plan doctor <plan> --kind quality` then `codeterm plan approve <plan>`\n",
            );
        }
    }

    if let Some(cov) = val.get("coverage") {
        let frac = cov
            .get("plan_covered_fraction")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            * 100.0;
        let gaps = cov
            .get("gaps")
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len());
        out.push_str(&format!("coverage: {frac:.0}% ({gaps} section gap(s))\n"));
        if val
            .get("task_counts")
            .and_then(|c| c.get("total"))
            .and_then(|v| v.as_u64())
            == Some(0)
        {
            out.push_str("coverage note: this is task coverage, so 0% is expected before execution tasks exist\n");
        }
    }

    if let Some(readiness) = val.get("planning_readiness") {
        let spec_frac = readiness
            .get("spec_link_coverage")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            * 100.0;
        let linked = u(readiness, "linked_plan_sections");
        let total = u(readiness, "total_plan_sections");
        out.push_str(&format!(
            "planning readiness: goal={} spec_source={} plan_sections={} spec_links={spec_frac:.0}% ({linked}/{total}) approval_gate={} verification={}\n",
            readiness
                .get("has_goal")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            readiness
                .get("has_spec_source")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            readiness
                .get("has_plan_sections")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            readiness
                .get("has_approval_gate_hint")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            readiness
                .get("has_verification_hint")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        ));
    }

    if let Some(drift) = val.get("drift") {
        let total = u(drift, "total");
        out.push_str(&format!("drift: {total}"));
        if let Some(by_kind) = drift.get("by_kind").and_then(|v| v.as_object()) {
            if !by_kind.is_empty() {
                let parts: Vec<String> = by_kind
                    .iter()
                    .map(|(k, v)| format!("{k} {}", v.as_u64().unwrap_or(0)))
                    .collect();
                out.push_str(&format!(" ({})", parts.join(", ")));
            }
        }
        out.push('\n');
    }

    render_regression_gates(val, &mut out);

    if let Some(hb) = val.get("heartbeat") {
        let focus = hb
            .get("current_focus")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .unwrap_or("-");
        out.push_str(&format!("focus: {focus}\n"));
        let lanes: Vec<&str> = hb
            .get("active_lanes")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if !lanes.is_empty() {
            out.push_str(&format!("active lanes: {}\n", lanes.join(", ")));
        }
    }

    if let Some(loops) = val.get("loops").and_then(|v| v.as_array()) {
        if !loops.is_empty() {
            out.push_str(&format!("loops: {}\n", loops.len()));
            for l in loops {
                out.push_str(&format!(
                    "  - {} [{}] {}: {}/{}{}\n",
                    s(l, "id"),
                    s(l, "status"),
                    s(l, "target"),
                    u(l, "count"),
                    u(l, "budget"),
                    l.get("stop_when")
                        .and_then(|v| v.as_str())
                        .filter(|v| !v.is_empty())
                        .map(|w| format!("; stop_when: {w}"))
                        .unwrap_or_default(),
                ));
            }
        }
    }

    if let Some(lanes) = val.get("open_lanes").and_then(|v| v.as_array()) {
        if !lanes.is_empty() {
            out.push_str(&format!("open lanes: {}\n", lanes.len()));
            for l in lanes {
                let role_part = l
                    .get("executor_role")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(|r| format!(" role:{r}"))
                    .unwrap_or_default();
                let competition_part = l
                    .get("competition")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(|c| {
                        let live = l
                            .get("live_competition_attempts")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        format!(" competition:{c} live:{live}")
                    })
                    .unwrap_or_default();
                let presentation = lane_presentation(l);
                out.push_str(&format!(
                    "  - {} [{}; {}]{}{} {}\n",
                    s(l, "task_id"),
                    s(l, "status"),
                    presentation,
                    role_part,
                    competition_part,
                    s(l, "name"),
                ));
            }
        }
    }

    if let Some(agents) = val.get("live_agents").and_then(|v| v.as_array()) {
        if !agents.is_empty() {
            out.push_str(&format!("live agents: {}\n", agents.len()));
            for a in agents {
                out.push_str(&format!(
                    "  - {} -> {} ({})\n",
                    s(a, "task_id"),
                    s(a, "tab_id"),
                    s(a, "name"),
                ));
            }
        }
    }

    if let Some(strides) = val.get("recent_strides").and_then(|v| v.as_array()) {
        if !strides.is_empty() {
            out.push_str(&format!("recent strides: {}\n", strides.len()));
            for m in strides {
                let text = s(m, "text");
                let kind = s(m, "kind");
                let scope = if m.get("plan_level").and_then(|v| v.as_bool()) == Some(true) {
                    " (plan)"
                } else {
                    ""
                };
                out.push_str(&format!("  - [{kind}]{scope} {text}\n"));
            }
        }
    }

    out
}

fn lane_presentation(lane: &Value) -> String {
    match s(lane, "presentation") {
        "handoff_required" => "handoff required: sealed checkpoint awaits adoption".into(),
        "actionable_pending" => "actionable pending".into(),
        "waiting_on_dependencies" => {
            let waits = lane
                .get("dependency_waits")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .map(|wait| s(wait, "task_id"))
                        .filter(|id| !id.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|ids| !ids.is_empty())
                .unwrap_or_else(|| "dependencies".into());
            format!("waiting on dependency: {waits}")
        }
        "queued_for_qa" => {
            let label = s(lane, "status_label").trim();
            if label.is_empty() {
                "queued for qa".into()
            } else {
                format!("queued for qa: {label}")
            }
        }
        "" => s(lane, "status").into(),
        other => other.replace('_', " "),
    }
}

#[cfg(test)]
mod tests {
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
    fn renders_blocking_regression_gates_and_override() {
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
        assert!(out.contains("dispatch: BLOCKED by regression gate(s): phase-2"));
        assert!(out.contains("override by tab_o @ 42 [phase-1]: hotfix under sign-off"));
    }

    #[test]
    fn renders_unblocked_when_no_blocking_gates() {
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
        assert!(out.contains("dispatch: unblocked"));
    }

    #[test]
    fn renders_pending_group_retrospective_as_a_dispatch_block() {
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
        assert!(!out.contains("dispatch: unblocked"));
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
}
