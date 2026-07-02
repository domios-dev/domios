use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::cmd::plan_local::{plan_files, plan_id_from, project_root};

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
    for task in &tasks {
        inspect_task_quality(path, plan, task, &mut findings);
    }
    findings
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
            task_id: Some(task_id),
            detail: format!("task is missing {}", missing.join(", ")),
            fix: "update the task with body, section links, criteria, test plan, and correctness before dispatch".into(),
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
mod tests {
    use super::*;

    fn run_git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn detects_missing_milestone_from() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "sections": [],
            "tasks": [{
                "id": "task_1",
                "name": "Task",
                "status": "done",
                "milestones": [{"id": "ms_1", "kind": "status", "ts": 1}]
            }]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "storage");
        assert!(findings
            .iter()
            .any(|f| f.kind == "missing_milestone_field" && f.detail.contains("'from'")));
    }

    #[test]
    fn detects_missing_parent_and_section() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "sections": [{"id": "sec_1", "title": "Spec"}],
            "tasks": [{
                "id": "task_1",
                "name": "Task",
                "status": "pending",
                "parent_task_id": "missing",
                "section_ids": ["missing_sec"]
            }]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "structure");
        assert!(findings.iter().any(|f| f.kind == "missing_parent_task"));
        assert!(findings.iter().any(|f| f.kind == "missing_section"));
    }

    #[test]
    fn detects_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let plan_dir = dir.path().join(".codeterm/plans/active/pl_bad");
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::write(plan_dir.join("plan.json"), "{not json").unwrap();
        let report = doctor_report(None, Some(dir.path().display().to_string()), "all").unwrap();
        assert!(!report["ok"].as_bool().unwrap());
        assert!(report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "malformed_json"));
    }

    #[test]
    fn storage_warns_about_stray_linked_worktree_sidecar_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let linked = tmp.path().join("linked");
        std::fs::create_dir(&main).unwrap();
        run_git(&main, &["init"]);
        run_git(&main, &["config", "user.email", "test@example.com"]);
        run_git(&main, &["config", "user.name", "Test User"]);
        std::fs::write(main.join("README.md"), "root\n").unwrap();
        run_git(&main, &["add", "README.md"]);
        run_git(&main, &["commit", "-m", "init"]);
        run_git(
            &main,
            &[
                "worktree",
                "add",
                linked.to_str().unwrap(),
                "-b",
                "linked-test",
            ],
        );

        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "owner": {},
            "project_path": main.display().to_string(),
            "canonical_project_path": main.display().to_string(),
            "created_at": 1,
            "updated_at": 2,
            "tasks": []
        });
        let canonical_dir = main.join(".codeterm/plans/active/pl_1");
        let stray_dir = linked.join(".codeterm/plans/active/pl_1");
        std::fs::create_dir_all(&canonical_dir).unwrap();
        std::fs::create_dir_all(&stray_dir).unwrap();
        let raw = serde_json::to_string_pretty(&plan).unwrap();
        std::fs::write(canonical_dir.join("plan.json"), &raw).unwrap();
        std::fs::write(stray_dir.join("plan.json"), raw).unwrap();

        let report = doctor_report(None, Some(linked.display().to_string()), "storage").unwrap();
        let findings = report["findings"].as_array().unwrap();
        assert!(findings.iter().any(|f| {
            f["kind"] == "stray_sidecar_copy"
                && f["detail"]
                    .as_str()
                    .unwrap()
                    .contains(main.to_str().unwrap())
                && f["path"]
                    .as_str()
                    .unwrap()
                    .contains(linked.to_str().unwrap())
        }));
    }

    #[test]
    fn detects_duplicate_handle_and_parent_cycle() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "sections": [],
            "tasks": [
                {
                    "id": "task_a",
                    "name": "A",
                    "status": "pending",
                    "handle": "dup",
                    "parent_task_id": "task_b"
                },
                {
                    "id": "task_b",
                    "name": "B",
                    "status": "pending",
                    "handle": "dup",
                    "parent_task_id": "task_a"
                }
            ]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "structure");
        assert!(findings.iter().any(|f| f.kind == "duplicate_task_handle"));
        assert!(findings.iter().any(|f| f.kind == "parent_cycle"));
    }

    #[test]
    fn renders_summary() {
        let report = json!({
            "checked_plans": 1,
            "error_count": 1,
            "warning_count": 0,
            "findings": [{
                "severity": "error",
                "kind": "missing_parent_task",
                "plan_id": "pl_1",
                "task_id": "task_1",
                "detail": "parent missing",
                "fix": "clear parent"
            }]
        });
        let rendered = render_doctor(&report);
        assert!(rendered.contains("Plan doctor: 1 checked, 1 errors"));
        assert!(rendered.contains("missing_parent_task"));
    }

    #[test]
    fn approval_error_includes_doctor_recovery_commands() {
        let report = json!({
            "checked_plans": 1,
            "error_count": 1,
            "warning_count": 0,
            "findings": [{
                "severity": "error",
                "kind": "missing_goal",
                "plan_id": "pl_1",
                "task_id": null,
                "detail": "plan has no goal vector",
                "fix": "set the north-star first"
            }]
        });
        let rendered = render_quality_approval_error("pl_1", &report);
        assert!(rendered.contains("plan approval blocked by quality doctor"));
        assert!(rendered.contains("codeterm plan doctor pl_1 --kind quality"));
        assert!(rendered.contains("codeterm plan approve pl_1"));
    }

    #[test]
    fn quality_blocks_unapproved_or_underplanned_work() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "sections": [{
                "id": "sec_1",
                "title": "Requirements",
                "body": "WHAT/WHY: evidence and accepted gap"
            }],
            "tasks": [],
            "heartbeat": {"current_focus": "remember policy"}
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
        let kinds = findings.iter().map(|f| f.kind).collect::<Vec<_>>();
        assert!(kinds.contains(&"missing_goal"));
        assert!(kinds.contains(&"missing_spec_source"));
        assert!(kinds.contains(&"heartbeat_without_execution"));
        assert!(kinds.contains(&"planning_mode_no_tasks"));
        assert!(findings
            .iter()
            .any(|f| f.kind == "missing_goal" && f.severity == "error"));
        assert!(findings
            .iter()
            .any(|f| f.kind == "planning_mode_no_tasks" && f.severity == "warning"));
    }

    #[test]
    fn planning_kind_tasks_do_not_trip_approval_gate() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "planning",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{"id": "sec_1", "title": "Spec", "body": "WHAT/WHY"}],
            "tasks": [{
                "id": "task_1",
                "name": "Improve task graph",
                "kind": "planning",
                "status": "pending",
                "section_ids": ["sec_1"]
            }]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
        assert!(
            !findings.iter().any(|f| f.kind == "tasks_before_approval"),
            "planning-kind tasks must not trip the approval gate"
        );
    }

    #[test]
    fn execution_kind_task_in_planning_mode_trips_approval_gate() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "planning",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{"id": "sec_1", "title": "Spec", "body": "WHAT/WHY"}],
            "tasks": [{
                "id": "task_1",
                "name": "Build feature",
                "kind": "implement",
                "status": "pending",
                "section_ids": ["sec_1"]
            }]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
        assert!(
            findings.iter().any(|f| f.kind == "tasks_before_approval"),
            "execution-kind tasks in planning mode must trip the approval gate"
        );
    }

    #[test]
    fn quality_errors_for_underspecified_tasks() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "approved",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{
                "id": "sec_1",
                "title": "File ownership and approval gate",
                "body": "Exact paths, verification with build.sh check."
            }],
            "tasks": [{
                "id": "task_1",
                "name": "Do work",
                "status": "pending",
                "section_ids": []
            }]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
        assert!(findings.iter().any(|f| f.kind == "task_quality_gap"
            && f.task_id.as_deref() == Some("task_1")
            && f.severity == "error"
            && f.detail.contains("acceptance criteria")));
    }

    #[test]
    fn quality_accepts_complete_task_and_plan_scaffold() {
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "approved",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{
            "id": "sec_1",
            "title": "Execution",
            "body": "File ownership map with exact paths. Approval gate. Verification map: build.sh check. Wave sequencing: discovery is intermediate, then synthesis creates remaining tasks."
        }],
            "tasks": [{
                "id": "task_1",
                "name": "Do work",
                "status": "pending",
                "section_ids": ["sec_1"],
                "body": "Implement the scoped change.",
                "acceptance_criteria": ["criterion"],
                "test_plan": "run focused test",
                "correctness": "matches spec"
            }]
        });
        let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn quality_accepts_externalized_task_body_via_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("do-work.md"),
            "---\nplan_id: pl_1\nkind: task\n---\n\nThis is the externalized task body content.\n",
        )
        .unwrap();
        let plan_path = dir.path().join("plan.json");
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "approved",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{"id": "sec_1", "title": "Execution", "body": "section body"}],
            "tasks": [{
                "id": "task_1",
                "name": "Do work",
                "status": "pending",
                "handle": "do-work",
                "section_ids": ["sec_1"],
                "body": "",
                "acceptance_criteria": ["criterion"],
                "test_plan": "run focused test",
                "correctness": "matches spec"
            }]
        });
        let findings = inspect_plan(&plan_path, &plan, "quality");
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn quality_errors_for_missing_body_without_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let plan_path = dir.path().join("plan.json");
        let plan = json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "approved",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{"id": "sec_1", "title": "Execution", "body": "section body"}],
            "tasks": [{
                "id": "task_1",
                "name": "Do work",
                "status": "pending",
                "handle": "do-work",
                "section_ids": ["sec_1"],
                "body": "",
                "acceptance_criteria": ["criterion"],
                "test_plan": "run focused test",
                "correctness": "matches spec"
            }]
        });
        let findings = inspect_plan(&plan_path, &plan, "quality");
        assert!(
            findings.iter().any(|f| f.kind == "task_quality_gap"
                && f.task_id.as_deref() == Some("task_1")
                && f.detail.contains("body")),
            "expected task_quality_gap for missing body: {findings:?}"
        );
    }
}
