use std::collections::{HashMap, HashSet};
use std::path::Path;

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
                Ok(plan) => findings.extend(inspect_plan(&path, &plan, kind)),
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

fn inspect_plan(path: &Path, plan: &Value, kind: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    if matches!(kind, "all" | "storage") {
        findings.extend(inspect_storage(path, plan));
    }
    if matches!(kind, "all" | "structure") {
        findings.extend(inspect_structure(path, plan));
    }
    if !matches!(kind, "all" | "storage" | "structure") {
        findings.push(Finding {
            severity: "error",
            kind: "unknown_doctor_kind",
            plan_id: plan_id(plan, path),
            path: path.display().to_string(),
            task_id: None,
            detail: format!("unknown doctor kind '{kind}'"),
            fix: "use --kind all, --kind storage, or --kind structure".into(),
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
        let plans_dir = dir.path().join(".codeterm/plans");
        std::fs::create_dir_all(&plans_dir).unwrap();
        std::fs::write(plans_dir.join("pl_bad.json"), "{not json").unwrap();
        let report = doctor_report(None, Some(dir.path().display().to_string()), "all").unwrap();
        assert!(!report["ok"].as_bool().unwrap());
        assert!(report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "malformed_json"));
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
}
