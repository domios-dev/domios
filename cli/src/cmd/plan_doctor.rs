use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cmd::plan_local::{plan_files, plan_id_from, project_root};

const STALLED_LOOP_AFTER_MS: u64 = 3 * 24 * 60 * 60 * 1000;
// Mirrors src-tauri/src/plan/reconcile.rs PENDING_UNSTAFFED_STALE_MS.
const PENDING_UNSTAFFED_STALE_MS: u64 = 4 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
struct Finding {
    severity: &'static str,
    kind: &'static str,
    plan_id: Option<String>,
    path: String,
    task_id: Option<String>,
    detail: String,
    fix: String,
}

impl Finding {
    fn json(&self) -> Value {
        json!({
            "severity": self.severity,
            "kind": self.kind,
            "plan_id": self.plan_id,
            "path": self.path,
            "task_id": self.task_id,
            "detail": self.detail,
            "fix": self.fix,
        })
    }
}

pub fn run(
    json_mode: bool,
    plan_id: Option<String>,
    project: Option<String>,
    kind: String,
) -> Result<Option<Value>, String> {
    let report = doctor_report(plan_id, project, &kind)?;
    let ok = report.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if json_mode {
        if !ok {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            std::process::exit(1);
        }
        return Ok(Some(report));
    }
    print!("{}", render_doctor(&report));
    if !ok {
        std::process::exit(1);
    }
    Ok(None)
}

pub(crate) fn assert_quality_ready(plan_id: &str) -> Result<(), String> {
    let report = doctor_report(Some(plan_id.to_string()), None, "quality")?;
    if report.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(());
    }
    Err(render_quality_approval_error(plan_id, &report))
}

fn render_quality_approval_error(plan_id: &str, report: &Value) -> String {
    format!(
        "✗ plan approval blocked by quality doctor\n\n{}fix:\n  codeterm plan doctor {plan_id} --kind quality\n  codeterm plan approve {plan_id}",
        render_doctor(&report)
    )
}

fn doctor_report(
    plan_id: Option<String>,
    project: Option<String>,
    kind: &str,
) -> Result<Value, String> {
    let project_root = project_root(project)?;
    let plans_dir = project_root.join(".codeterm").join("plans");
    let files = plan_files(&plans_dir, plan_id.as_deref())?;
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for path in files {
        checked += 1;
        match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(plan) => {
                    findings.extend(inspect_plan(&path, &plan, kind));
                    if matches!(kind, "all" | "storage") {
                        findings.extend(inspect_stray_sidecars(&project_root, &path, &plan));
                    }
                }
                Err(err) => findings.push(Finding {
                    severity: "error",
                    kind: "malformed_json",
                    plan_id: path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string),
                    path: path.display().to_string(),
                    task_id: None,
                    detail: err.to_string(),
                    fix: "open the JSON file, repair syntax, then rerun plan doctor".into(),
                }),
            },
            Err(err) => findings.push(Finding {
                severity: "error",
                kind: "unreadable_file",
                plan_id: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string),
                path: path.display().to_string(),
                task_id: None,
                detail: err.to_string(),
                fix: "check file permissions or remove the stale plan entry".into(),
            }),
        }
    }
    let error_count = findings.iter().filter(|f| f.severity == "error").count();
    Ok(json!({
        "ok": error_count == 0,
        "project": project_root.display().to_string(),
        "kind": kind,
        "checked_plans": checked,
        "error_count": error_count,
        "warning_count": findings.iter().filter(|f| f.severity == "warning").count(),
        "findings": findings.iter().map(Finding::json).collect::<Vec<_>>(),
    }))
}

fn inspect_stray_sidecars(
    project_root: &Path,
    canonical_path: &Path,
    plan: &Value,
) -> Vec<Finding> {
    let Some(plan_id) = plan_id_from(plan, canonical_path) else {
        return vec![];
    };
    let canonical_project = canonical_project_path(project_root, plan);
    let canonical_plan = canonical_path
        .canonicalize()
        .unwrap_or_else(|_| canonical_path.to_path_buf());
    worktree_roots(project_root)
        .into_iter()
        .filter(|root| canonicalize_path(root) != canonicalize_path(&canonical_project))
        .flat_map(|root| {
            let plan_id = plan_id.clone();
            ["active", "archive"]
                .into_iter()
                .map(move |bucket| {
                    root.join(".codeterm")
                        .join("plans")
                        .join(bucket)
                        .join(plan_stem(&plan_id))
                        .join("plan.json")
                })
        })
        .filter(|path| path.is_file())
        .filter(|path| canonicalize_path(path) != canonical_plan)
        .map(|path| Finding {
            severity: "warning",
            kind: "stray_sidecar_copy",
            plan_id: Some(plan_id.clone()),
            path: path.display().to_string(),
            task_id: None,
            detail: format!(
                "non-canonical plan sidecar copy; canonical project path is {}",
                canonical_project.display()
            ),
            fix: "remove the stray .codeterm/plans copy from the linked worktree after confirming canonical state".into(),
        })
        .collect()
}

fn canonical_project_path(project_root: &Path, plan: &Value) -> PathBuf {
    plan.get("canonical_project_path")
        .or_else(|| plan.get("project_path"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| project_root.to_path_buf())
}

fn worktree_roots(project_root: &Path) -> Vec<PathBuf> {
    let Ok(out) = Command::new("git")
        .current_dir(project_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
    else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect()
}

fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn plan_stem(id: &str) -> &str {
    if id == "inbox" {
        "_inbox"
    } else {
        id
    }
}

fn inspect_plan(path: &Path, plan: &Value, kind: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    if matches!(kind, "all" | "storage") {
        findings.extend(inspect_storage(path, plan));
    }
    if matches!(kind, "all" | "structure") {
        findings.extend(inspect_structure(path, plan));
    }
    if matches!(kind, "all" | "quality") {
        findings.extend(inspect_quality(path, plan));
    }
    if !matches!(kind, "all" | "storage" | "structure" | "quality") {
        findings.push(Finding {
            severity: "error",
            kind: "unknown_doctor_kind",
            plan_id: plan_id(plan, path),
            path: path.display().to_string(),
            task_id: None,
            detail: format!("unknown doctor kind '{kind}'"),
            fix: "use --kind all, --kind storage, --kind structure, or --kind quality".into(),
        });
    }
    findings
}

fn inspect_storage(path: &Path, plan: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    for key in [
        "id",
        "title",
        "status",
        "owner",
        "created_at",
        "updated_at",
        "tasks",
    ] {
        if plan.get(key).is_none() {
            findings.push(Finding {
                severity: "error",
                kind: "missing_plan_field",
                plan_id: plan_id(plan, path),
                path: path.display().to_string(),
                task_id: None,
                detail: format!("plan is missing required field '{key}'"),
                fix: "repair the plan JSON or restore it from git".into(),
            });
        }
    }
    let Some(tasks) = plan.get("tasks").and_then(Value::as_array) else {
        return findings;
    };
    for task in tasks {
        let task_id = task.get("id").and_then(Value::as_str).map(str::to_string);
        for key in ["id", "name", "type", "status"] {
            if task.get(key).is_none() {
                findings.push(Finding {
                    severity: "error",
                    kind: "missing_task_field",
                    plan_id: plan_id(plan, path),
                    path: path.display().to_string(),
                    task_id: task_id.clone(),
                    detail: format!("task is missing required field '{key}'"),
                    fix: "repair the task object before the daemon can safely load it".into(),
                });
            }
        }
        if let Some(milestones) = task.get("milestones").and_then(Value::as_array) {
            for milestone in milestones {
                for key in ["id", "from", "kind", "ts"] {
                    if milestone.get(key).is_none() {
                        findings.push(Finding {
                            severity: "error",
                            kind: "missing_milestone_field",
                            plan_id: plan_id(plan, path),
                            path: path.display().to_string(),
                            task_id: task_id.clone(),
                            detail: format!("milestone is missing required field '{key}'"),
                            fix: "rewrite the milestone through plan milestone/record or add the missing schema field".into(),
                        });
                    }
                }
            }
        }
    }
    findings
}

fn inspect_structure(path: &Path, plan: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let section_ids = collect_section_ids(plan.get("sections").and_then(Value::as_array));
    let tasks = plan
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let task_ids = tasks
        .iter()
        .filter_map(|t| t.get("id").and_then(Value::as_str))
        .collect::<HashSet<_>>();
    let parent_by_task = tasks
        .iter()
        .filter_map(|t| {
            Some((
                t.get("id")?.as_str()?.to_string(),
                t.get("parent_task_id")?.as_str()?.to_string(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut handles = HashMap::<String, String>::new();
    for task in &tasks {
        let task_id = task
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        if let Some(parent) = task.get("parent_task_id").and_then(Value::as_str) {
            if !task_ids.contains(parent) {
                findings.push(structure_finding(
                    path,
                    plan,
                    &task_id,
                    "missing_parent_task",
                    format!("parent_task_id '{parent}' does not exist"),
                    "clear --parent or point it at an existing task",
                ));
            }
        }
        for section in task
            .get("section_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(section_id) = section.as_str() {
                if !section_ids.contains(section_id) {
                    findings.push(structure_finding(
                        path,
                        plan,
                        &task_id,
                        "missing_section",
                        format!("section_id '{section_id}' does not exist"),
                        "relink the task to an existing section or remove the stale section id",
                    ));
                }
            }
        }
        if let Some(handle) = task.get("handle").and_then(Value::as_str) {
            if let Some(previous) = handles.insert(handle.to_string(), task_id.clone()) {
                findings.push(structure_finding(
                    path,
                    plan,
                    &task_id,
                    "duplicate_task_handle",
                    format!("handle '{handle}' is also used by task {previous}"),
                    "retag or rename one task so handles are unique",
                ));
            }
        }
    }
    for task_id in parent_by_task.keys() {
        if has_parent_cycle(task_id, &parent_by_task) {
            findings.push(structure_finding(
                path,
                plan,
                task_id,
                "parent_cycle",
                "parent_task_id chain cycles".into(),
                "clear one parent link in the cycle",
            ));
        }
    }
    findings
}

fn collect_section_ids(sections: Option<&Vec<Value>>) -> HashSet<String> {
    let mut ids = HashSet::new();
    fn walk(section: &Value, ids: &mut HashSet<String>) {
        if let Some(id) = section.get("id").and_then(Value::as_str) {
            ids.insert(id.to_string());
        }
        if let Some(children) = section.get("subsections").and_then(Value::as_array) {
            for child in children {
                walk(child, ids);
            }
        }
    }
    if let Some(sections) = sections {
        for section in sections {
            walk(section, &mut ids);
        }
    }
    ids
}

fn has_parent_cycle(task_id: &str, parent_by_task: &HashMap<String, String>) -> bool {
    let mut seen = HashSet::new();
    let mut current = task_id;
    while let Some(parent) = parent_by_task.get(current) {
        if !seen.insert(parent.as_str()) {
            return true;
        }
        current = parent;
    }
    false
}

fn structure_finding(
    path: &Path,
    plan: &Value,
    task_id: &str,
    kind: &'static str,
    detail: String,
    fix: &str,
) -> Finding {
    Finding {
        severity: "error",
        kind,
        plan_id: plan_id(plan, path),
        path: path.display().to_string(),
        task_id: Some(task_id.into()),
        detail,
        fix: fix.into(),
    }
}

fn inspect_quality(path: &Path, plan: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mode = plan
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("planning");
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
    // Planning-kind tasks are the planning workflow itself, not premature execution work,
    // so they never trip the approval gate; only non-planning tasks do.
    let non_planning_tasks = tasks
        .iter()
        .filter(|t| t.get("kind").and_then(Value::as_str) != Some("planning"))
        .count();
    if mode == "planning" && non_planning_tasks > 0 {
        findings.push(plan_quality_error(
            path,
            plan,
            "tasks_before_approval",
            "planning-mode plan already has tasks",
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
    findings.extend(inspect_loop_quality(path, plan, now_ms));
    for task in &tasks {
        inspect_task_quality(path, plan, task, &mut findings);
        inspect_task_lifecycle(path, plan, task, now_ms, &mut findings);
    }
    findings
}

fn inspect_task_lifecycle(
    path: &Path,
    plan: &Value,
    task: &Value,
    now_ms: u64,
    findings: &mut Vec<Finding>,
) {
    let task_id = task.get("id").and_then(Value::as_str).map(str::to_string);
    let status = task.get("status").and_then(Value::as_str).unwrap_or("");
    if status == "parked" {
        let has_reason = task
            .get("park_reason")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_reason {
            findings.push(Finding {
                severity: "error",
                kind: "parked_without_reason",
                plan_id: plan_id(plan, path),
                path: path.display().to_string(),
                task_id: task_id.clone(),
                detail: "task is parked but has no park_reason".into(),
                fix: "unpark and re-park with a reason: `codeterm plan task park <plan> <task> --reason \"<why>\"`".into(),
            });
        }
    }
    let staffed = task
        .get("assigned_pane_id")
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
            "dispatch it, park it with a reason, or send it to replanning",
        ));
    }
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
    let mut missing = Vec::new();
    if task
        .get("section_ids")
        .and_then(Value::as_array)
        .is_none_or(|items| items.is_empty())
    {
        missing.push("section link");
    }
    if task
        .get("body")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
        && !task_has_body_sidecar(path, task)
    {
        missing.push("body");
    }
    if task
        .get("acceptance_criteria")
        .and_then(Value::as_array)
        .is_none_or(|items| items.is_empty())
    {
        missing.push("acceptance criteria");
    }
    if task
        .get("test_plan")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        missing.push("test plan");
    }
    if task
        .get("correctness")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        missing.push("correctness vector");
    }
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
    // executor_role is advisory for execution-kind tasks; planning-kind tasks are exempt
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
            task_id: Some(task_id),
            detail: "execution task has no executor_role; orchestrator cannot route agent personality declaratively".into(),
            fix: "set --executor-role <slug> (e.g. rust-backend-dev, frontend-dev, code-reviewer) on the task".into(),
        });
    }
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

fn plan_id(plan: &Value, path: &Path) -> Option<String> {
    plan_id_from(plan, path)
}

pub(crate) fn render_doctor(report: &Value) -> String {
    let findings = report
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = format!(
        "Plan doctor: {} checked, {} errors, {} warnings\n",
        report
            .get("checked_plans")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        report
            .get("error_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        report
            .get("warning_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    for finding in findings {
        let severity = finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let kind = finding.get("kind").and_then(Value::as_str).unwrap_or("?");
        let plan = finding
            .get("plan_id")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let task = finding.get("task_id").and_then(Value::as_str).unwrap_or("");
        let detail = finding.get("detail").and_then(Value::as_str).unwrap_or("");
        let fix = finding.get("fix").and_then(Value::as_str).unwrap_or("");
        let task_suffix = if task.is_empty() {
            "".into()
        } else {
            format!(" task={task}")
        };
        out.push_str(&format!(
            "  - {severity} {kind} plan={plan}{task_suffix}: {detail}; fix: {fix}\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests;
