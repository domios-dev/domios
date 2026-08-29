use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::cleanup;
use super::{plan_id, Finding, PENDING_UNSTAFFED_STALE_MS, STALLED_LOOP_AFTER_MS};

pub(super) fn inspect_quality(path: &Path, plan: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let unlocked = execution_tasks_unlocked(plan);
    if plan
        .get("goal")
        .and_then(|g| g.get("vector"))
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        findings.push(plan_quality_error(
            path,
            plan,
            "missing_goal",
            "plan has no goal vector",
            "set the north-star first: codeterm plan goal set <plan> --vector <objective>",
        ));
    }
    if !has_spec_source(plan) {
        findings.push(plan_quality_error(
            path,
            plan,
            "missing_spec_source",
            "plan is not linked to a spec",
            "create/update the spec first, then use codeterm plan create --from-spec or spec link add",
        ));
    }
    if let Some(finding) = inspect_planner_pairing(path, plan) {
        findings.push(finding);
    }

    let tasks = plan
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let sections = plan.get("sections").and_then(Value::as_array);
    let section_text = flatten_section_text(sections);
    if tasks.is_empty() && heartbeat_has_content(plan) {
        findings.push(plan_quality_error(
            path,
            plan,
            "heartbeat_without_execution",
            "heartbeat has runtime state while the plan has no tasks",
            "move planning-only policy into spec/plan sections or mem; keep heartbeat for active lanes",
        ));
    }
    // so they never trip the approval gate; only non-planning tasks do.
    let non_planning_tasks = tasks.iter().filter(|t| !task_kind_is_planning(t)).count();
    if !unlocked && non_planning_tasks > 0 {
        findings.push(plan_quality_error(
            path,
            plan,
            "tasks_before_approval",
            "unapproved plan already has execution tasks",
            "run the approval gate and `codeterm plan approve <plan>`, or remove draft tasks with `codeterm plan task rm <plan> --all --yes`",
        ));
    }
    if tasks.is_empty() && !section_text.is_empty() {
        findings.push(plan_warning(
            path,
            plan,
            "planning_mode_no_tasks",
            "plan has sections but no tasks; task coverage will read as 0%",
            "this is fine before approval; run quality review before generating tasks",
        ));
    }
    findings.extend(inspect_duplicate_role_guidance(path, plan, &tasks));
    findings.extend(inspect_loop_quality(path, plan, now_ms));
    for task in &tasks {
        inspect_task_quality(path, plan, task, &mut findings);
        inspect_task_lifecycle(path, plan, task, now_ms, &mut findings);
    }
    findings
}

fn inspect_duplicate_role_guidance(path: &Path, plan: &Value, tasks: &[Value]) -> Vec<Finding> {
    let mut task_ids_by_guidance: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for task in tasks {
        let Some(guidance) = task
            .get("role_guidance")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|guidance| !guidance.is_empty())
        else {
            continue;
        };
        let task_id = task.get("id").and_then(Value::as_str).unwrap_or("?");
        task_ids_by_guidance
            .entry(guidance)
            .or_default()
            .push(task_id);
    }

    task_ids_by_guidance
        .into_values()
        .filter(|task_ids| task_ids.len() > 1)
        .map(|task_ids| Finding {
            severity: "warning",
            kind: "duplicate_role_guidance",
            plan_id: plan_id(plan, path),
            path: path.display().to_string(),
            task_id: None,
            detail: format!(
                "tasks {} carry identical role_guidance; confirm that their worker conditioning is intentionally shared",
                task_ids.join(", ")
            ),
            fix: "give each worker task-specific conditioning, or keep the duplicate and proceed when sharing is deliberate".into(),
        })
        .collect()
}

// authoritative; an approved/execution lifecycle mode bridges pre-state plans.
fn execution_tasks_unlocked(plan: &Value) -> bool {
    let mode = plan
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("planning");
    if mode == "complete" {
        return false;
    }
    let planning_state = plan
        .get("planning_state")
        .and_then(Value::as_str)
        .unwrap_or("drafting");
    planning_state == "approved" || mode == "approved" || mode == "execution"
}

pub(super) fn inspect_task_lifecycle(
    path: &Path,
    plan: &Value,
    task: &Value,
    now_ms: u64,
    findings: &mut Vec<Finding>,
) {
    let task_id = task.get("id").and_then(Value::as_str).map(str::to_string);
    let status = task.get("status").and_then(Value::as_str).unwrap_or("");
    if status == "paused" {
        let has_reason = task
            .get("pause_reason")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_reason {
            findings.push(Finding {
                severity: "error",
                kind: "paused_without_reason",
                plan_id: plan_id(plan, path),
                path: path.display().to_string(),
                task_id: task_id.clone(),
                detail: "task is paused but has no pause_reason".into(),
                fix: "resume and pause with a reason: `codeterm plan task pause <plan> <task> --reason \"<why>\"`".into(),
            });
        }
    }
    let staffed = task
        .get("assigned_tab_id")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    let changed_at = task
        .get("status_changed_at")
        .and_then(Value::as_u64)
        .filter(|&v| v > 0)
        .or_else(|| task.get("created_at").and_then(Value::as_u64))
        .unwrap_or(0);
    if status == "pending"
        && !staffed
        && changed_at > 0
        && now_ms.saturating_sub(changed_at) > PENDING_UNSTAFFED_STALE_MS
    {
        findings.push(plan_warning_task(
            path,
            plan,
            task_id,
            "aged_unstaffed_task",
            "pending task has been unstaffed for over 4h",
            "dispatch it, pause it with a reason, or send it to replanning",
        ));
    }
    findings.extend(cleanup::cleanup_debt_findings(path, plan, task));
}

fn plan_warning_task(
    path: &Path,
    plan: &Value,
    task_id: Option<String>,
    kind: &'static str,
    detail: &str,
    fix: &str,
) -> Finding {
    Finding {
        severity: "warning",
        kind,
        plan_id: plan_id(plan, path),
        path: path.display().to_string(),
        task_id,
        detail: detail.into(),
        fix: fix.into(),
    }
}

fn inspect_loop_quality(path: &Path, plan: &Value, now_ms: u64) -> Vec<Finding> {
    let Some(loops) = plan.get("loops").and_then(Value::as_array) else {
        return vec![];
    };
    let plan_updated_at = plan.get("updated_at").and_then(Value::as_u64).unwrap_or(0);
    let mut findings = Vec::new();
    for loop_spec in loops {
        let Some(loop_id) = loop_spec.get("id").and_then(Value::as_str) else {
            continue;
        };
        let status = loop_spec
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running");
        let count = loop_spec.get("count").and_then(Value::as_u64).unwrap_or(0);
        let budget = loop_spec.get("budget").and_then(Value::as_u64).unwrap_or(0);
        if status == "running" && budget > 0 && count >= budget {
            findings.push(plan_warning(
                path,
                plan,
                "exhausted_loop_still_running",
                &format!("loop {loop_id} is still marked running at {count}/{budget}"),
                &format!(
                    "record the stop or repair the loop status: codeterm plan loop stop {loop_id}"
                ),
            ));
        }
        if status != "running" {
            continue;
        }
        let last_iteration_at = loop_spec
            .get("last_iteration_at")
            .and_then(Value::as_u64)
            .unwrap_or(plan_updated_at);
        if last_iteration_at == 0
            || now_ms.saturating_sub(last_iteration_at) < STALLED_LOOP_AFTER_MS
        {
            continue;
        }
        findings.push(plan_warning(
            path,
            plan,
            "stalled_running_loop",
            &format!(
                "loop {loop_id} has been running without a recorded iteration for more than 3 days"
            ),
            &format!(
                "record a fresh iteration with codeterm plan loop record {loop_id} --note \"...\" or stop it explicitly"
            ),
        ));
    }
    findings
}

fn task_kind_is_planning(task: &Value) -> bool {
    task.get("type")
        .and_then(Value::as_str)
        .map(|k| matches!(k, "planning" | "plan" | "spec" | "design"))
        .unwrap_or(false)
}

fn inspect_task_quality(path: &Path, plan: &Value, task: &Value, findings: &mut Vec<Finding>) {
    let task_id = task
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    let has_section_link = !task
        .get("section_ids")
        .and_then(Value::as_array)
        .is_none_or(|items| items.is_empty());
    let has_body = !(task
        .get("body")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
        && !task_has_body_sidecar(path, task));
    let has_criteria = !task
        .get("acceptance_criteria")
        .and_then(Value::as_array)
        .is_none_or(|items| items.is_empty());
    let has_test_plan = !task
        .get("test_plan")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty());
    let has_correctness = !task
        .get("correctness")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty());

    // Same field list `plan task add` warns on at creation (crate::cmd::hints);
    // section link is doctor-only, tracked separately from that shared set.
    let mut missing: Vec<&str> = Vec::new();
    if !has_section_link {
        missing.push("section link");
    }
    missing.extend(crate::cmd::hints::missing_task_quality_fields(
        has_body,
        has_criteria,
        has_test_plan,
        has_correctness,
    ));
    if !missing.is_empty() {
        findings.push(Finding {
            severity: "error",
            kind: "task_quality_gap",
            plan_id: plan_id(plan, path),
            path: path.display().to_string(),
            task_id: Some(task_id.clone()),
            detail: format!("task is missing {}", missing.join(", ")),
            fix: "update the task with body, section links, criteria, test plan, and correctness before dispatch".into(),
        });
    }
    if !task_kind_is_planning(task) {
        if let Some(test_plan) = task
            .get("test_plan")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            if let Some(reason) = crate::cmd::test_plan::test_plan_vague_reason(test_plan) {
                findings.push(Finding {
                    severity: "warning",
                    kind: "test_plan_vague",
                    plan_id: plan_id(plan, path),
                    path: path.display().to_string(),
                    task_id: Some(task_id.clone()),
                    detail: format!(
                        "test plan is vague — {reason}; workers will fill the gaps with guesses"
                    ),
                    fix: "write an executable plan: exact command(s), exact surfaces (testids/selectors/files/endpoints), numbered steps with observable pass conditions".into(),
                });
            }
        }
    }
    if !task_kind_is_planning(task)
        && task
            .get("executor_role")
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
    {
        findings.push(Finding {
            severity: "warning",
            kind: "missing_executor_role",
            plan_id: plan_id(plan, path),
            path: path.display().to_string(),
            task_id: Some(task_id.clone()),
            detail: "execution task has no executor_role; orchestrator cannot route agent personality declaratively. A task carries no marker of the contract it was created under, so this check cannot tell a task created before `plan task add` required a role from one created after and still missing it — both look identical here".into(),
            fix: "set --executor-role <slug> (e.g. rust-backend-dev, frontend-dev, code-reviewer) on the task. Nothing fills this in for you: an invented role is worse than an absent one, because the Orchestrator would route on it".into(),
        });
    }
    inspect_test_author_report(path, plan, task, &task_id, findings);
    crate::cmd::plan_doctor_competition::push_competition_findings(
        task,
        plan_id(plan, path),
        path,
        task_id,
        findings,
    );
}

fn inspect_test_author_report(
    path: &Path,
    plan: &Value,
    task: &Value,
    task_id: &str,
    findings: &mut Vec<Finding>,
) {
    let is_test_author = task
        .get("executor_role")
        .and_then(Value::as_str)
        .is_some_and(|s| s.trim() == "test-author");
    if !is_test_author {
        return;
    }
    let Some(report) = task
        .get("lane")
        .and_then(|lane| lane.get("report"))
        .filter(|v| !v.is_null())
    else {
        return;
    };
    let report_text = report
        .get("message")
        .or_else(|| report.get("body"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if mentions_red_green_demo(report_text) {
        return;
    }
    findings.push(Finding {
        severity: "error",
        kind: "test_author_report_missing_red_green_demo",
        plan_id: plan_id(plan, path),
        path: path.display().to_string(),
        task_id: Some(task_id.to_string()),
        detail: "test-author task report lacks a red-green demo".into(),
        fix: "bounce the report or amend it with the failing run, green run, exact command(s), and the oracle that failed for the right reason".into(),
    });
}

fn mentions_red_green_demo(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("red-green")
        || lower.contains("red green")
        || (lower.contains("red:") && lower.contains("green:"))
}

fn task_has_body_sidecar(plan_path: &Path, task: &Value) -> bool {
    let Some(plan_dir) = plan_path.parent() else {
        return false;
    };
    let handle = task
        .get("handle")
        .and_then(Value::as_str)
        .or_else(|| task.get("id").and_then(Value::as_str));
    let Some(handle) = handle else {
        return false;
    };
    let sidecar = plan_dir.join("tasks").join(format!("{handle}.md"));
    sidecar_body_non_empty(&sidecar)
}

fn sidecar_body_non_empty(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    !strip_sidecar_frontmatter(&text).trim().is_empty()
}

fn strip_sidecar_frontmatter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text;
    };
    let Some(end) = rest.find("\n---") else {
        return text;
    };
    rest[end + "\n---".len()..].trim_start_matches('\n')
}

fn inspect_planner_pairing(path: &Path, plan: &Value) -> Option<Finding> {
    let has_orchestrator = plan
        .get("owner")
        .and_then(|o| o.get("tab_id"))
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    if !has_orchestrator {
        return None;
    }
    let planner_bound = plan.get("planner").is_some_and(|v| !v.is_null());
    if planner_bound {
        return None;
    }
    let plan_id_str = plan_id(plan, path).unwrap_or_else(|| "<plan>".into());
    Some(plan_warning(
        path,
        plan,
        "planner_unpaired",
        "orchestrator has no attached Planner",
        &format!(
            "choose an available provider and model for planning, then spawn: codeterm agent spawn <provider> [--model <model>] --parent-tab <orch-tab> --role Planner --plan {plan_id_str}; or waive: codeterm plan update {plan_id_str} --planner none --note \"<why>\""
        ),
    ))
}

fn has_spec_source(plan: &Value) -> bool {
    let has_primary = plan
        .get("primary_spec_id")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    let has_refs = plan
        .get("spec_refs")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    let has_links = plan
        .get("spec_links")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    has_primary || has_refs || has_links
}

fn heartbeat_has_content(plan: &Value) -> bool {
    let Some(hb) = plan.get("heartbeat") else {
        return false;
    };
    hb.get("current_focus")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
        || hb
            .get("active_lanes")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        || hb
            .get("rules")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        || hb
            .get("learnings")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
}

fn flatten_section_text(sections: Option<&Vec<Value>>) -> String {
    fn walk(section: &Value, out: &mut String) {
        for key in ["title", "body"] {
            if let Some(value) = section.get(key).and_then(Value::as_str) {
                out.push_str(value);
                out.push('\n');
            }
        }
        if let Some(children) = section.get("subsections").and_then(Value::as_array) {
            for child in children {
                walk(child, out);
            }
        }
    }
    let mut out = String::new();
    if let Some(sections) = sections {
        for section in sections {
            walk(section, &mut out);
        }
    }
    out.to_lowercase()
}

fn plan_quality_error(
    path: &Path,
    plan: &Value,
    kind: &'static str,
    detail: &str,
    fix: &str,
) -> Finding {
    Finding {
        severity: "error",
        kind,
        plan_id: plan_id(plan, path),
        path: path.display().to_string(),
        task_id: None,
        detail: detail.into(),
        fix: fix.into(),
    }
}

fn plan_warning(path: &Path, plan: &Value, kind: &'static str, detail: &str, fix: &str) -> Finding {
    Finding {
        severity: "warning",
        kind,
        plan_id: plan_id(plan, path),
        path: path.display().to_string(),
        task_id: None,
        detail: detail.into(),
        fix: fix.into(),
    }
}
