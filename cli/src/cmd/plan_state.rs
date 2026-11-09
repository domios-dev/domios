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

// Group gate records remain visible while dispatch stays available.
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
            "group retrospectives pending: {}\n",
            retrospectives.join(", ")
        ));
    }
    let mut advisory = if blocking.is_empty() {
        pending.clone()
    } else {
        blocking.clone()
    };
    advisory.extend(retrospectives);
    advisory.sort();
    advisory.dedup();
    if !advisory.is_empty() {
        out.push_str(&format!(
            "dispatch: allowed; terminal-group gate advisory: {}\n",
            advisory.join(", ")
        ));
    } else {
        out.push_str("dispatch: allowed (no unwaived terminal-group advisory)\n");
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
                "planning: delegate structure drafting to the Planner; run `domios plan doctor <plan> --kind quality` then `domios plan approve <plan>`\n",
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
            let total = val
                .pointer("/details/totals/open_lanes")
                .and_then(Value::as_u64)
                .unwrap_or(lanes.len() as u64);
            out.push_str(&format!("open lanes: {total} ({} shown)\n", lanes.len()));
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
                if let Some(children) = l
                    .get("subtasks")
                    .and_then(Value::as_array)
                    .filter(|items| !items.is_empty())
                {
                    let statuses = children
                        .iter()
                        .map(|child| {
                            format!(
                                "{} [{}]",
                                child.get("name").and_then(Value::as_str).unwrap_or("?"),
                                child.get("status").and_then(Value::as_str).unwrap_or("?")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!("    subtasks: {statuses}\n"));
                }
            }
        }
    }

    if let Some(agents) = val.get("live_agents").and_then(|v| v.as_array()) {
        if !agents.is_empty() {
            let total = val
                .pointer("/details/totals/live_agents")
                .and_then(Value::as_u64)
                .unwrap_or(agents.len() as u64);
            out.push_str(&format!("live agents: {total} ({} shown)\n", agents.len()));
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

    out.push_str(&render_detail_windows(val));
    out
}

pub fn render_detail_windows(val: &Value) -> String {
    val.pointer("/details/windows")
        .and_then(Value::as_object)
        .map(|windows| {
            windows
                .iter()
                .filter_map(|(name, window)| {
                    window
                        .get("hint")
                        .and_then(Value::as_str)
                        .map(|hint| format!("hint: {name}: {hint}\n"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lane_presentation(lane: &Value) -> String {
    match s(lane, "presentation") {
        "handoff_required" => "handoff required: sealed checkpoint awaits adoption".into(),
        "blocked_on_subtasks" => "blocked on subtasks".into(),
        "parked_for_later_group" => "parked for later groups".into(),
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
mod tests;
