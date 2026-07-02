use serde_json::Value;

fn s<'a>(val: &'a Value, key: &str) -> &'a str {
    val.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn u(val: &Value, key: &str) -> u64 {
    val.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
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
        .and_then(|o| o.get("pane_id"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty());
    if let Some(owner) = owner {
        out.push_str(&format!("owner: {owner}\n"));
    }

    if let Some(c) = val.get("task_counts") {
        let total = u(c, "total");
        let pct = val
            .get("percent_done")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            * 100.0;
        out.push_str(&format!(
            "tasks: {} total | done {} | in_progress {} | in_review {} | pending {} | blocked {} | cancelled {} ({pct:.0}% done)\n",
            total,
            u(c, "done"),
            u(c, "in_progress"),
            u(c, "in_review"),
            u(c, "pending"),
            u(c, "blocked"),
            u(c, "cancelled"),
        ));
        if total == 0 || mode == "planning" {
            out.push_str(
                "planning: finish spec/plan review before task generation; run `codeterm plan doctor <plan> --kind quality` then `codeterm plan approve <plan>`\n",
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
                out.push_str(&format!(
                    "  - {} [{}]{} {}\n",
                    s(l, "task_id"),
                    s(l, "status"),
                    role_part,
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
                    s(a, "pane_id"),
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
                out.push_str(&format!("  - [{kind}] {text}\n"));
            }
        }
    }

    out
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
            "owner": {"pane_id": "pane_o"},
            "task_counts": {"total": 4, "done": 1, "in_progress": 1, "in_review": 1, "pending": 1, "blocked": 0, "cancelled": 0},
            "percent_done": 0.25,
            "coverage": {"plan_covered_fraction": 0.5, "gaps": ["s2"]},
            "drift": {"total": 1, "by_kind": {"in_progress_pane_gone": 1}},
            "heartbeat": {"current_focus": "foundation", "active_lanes": ["loop-state"]},
            "loops": [{"id": "lp_1", "status": "running", "target": "harden", "count": 2, "budget": 5, "stop_when": "0 bugs"}],
            "open_lanes": [{"task_id": "t_p", "status": "pending", "name": "pend"}],
            "live_agents": [{"task_id": "t_r", "pane_id": "pane_live", "name": "rev"}],
            "recent_strides": [{"kind": "decision", "text": "did a thing"}]
        });
        let out = render_state(&val);
        assert!(out.contains("Loop [pl_1] (active)"));
        assert!(out.contains("goal: ship it"));
        assert!(out.contains("25% done"));
        assert!(out.contains("coverage: 50% (1 section gap"));
        assert!(out.contains("drift: 1 (in_progress_pane_gone 1)"));
        assert!(out.contains("focus: foundation"));
        assert!(out.contains("lp_1 [running] harden: 2/5; stop_when: 0 bugs"));
        assert!(out.contains("t_p [pending] pend"));
        assert!(out.contains("t_r -> pane_live (rev)"));
        assert!(out.contains("[decision] did a thing"));
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
        assert!(out.contains("planning: finish spec/plan review"));
        assert!(out.contains("coverage note: this is task coverage"));
        assert!(out.contains("planning readiness: goal=true"));
    }
}
