use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::cmd::plan_local::plan_id_from;

use super::{plan_id, Finding};

// Mirrors src-tauri prompts::REGRESSION_MILESTONE_KIND, reflection::GROUP_CLOSE_MILESTONE_KIND and milestone_gates::GROUP_REGRESSION_LINK_PREFIX.
const REGRESSION_MILESTONE_KIND: &str = "regression";
const GROUP_CLOSE_MILESTONE_KIND: &str = "group_close";
const GROUP_REGRESSION_LINK_PREFIX: &str = "group:";

pub(super) fn inspect_stray_sidecars(
    project_root: &Path,
    canonical_path: &Path,
    plan: &Value,
) -> Vec<Finding> {
    let Some(plan_id) = plan_id_from(plan, canonical_path) else {
        return vec![];
    };
    let canonical_project = canonicalize_path(&canonical_project_path(project_root, plan));
    let canonical_plan = canonicalize_path(canonical_path);
    let canonical_project_key = path_key(&canonical_project);
    let canonical_plan_key = path_key(&canonical_plan);
    worktree_roots(project_root)
        .into_iter()
        .map(|root| canonicalize_path(&root))
        .filter(|root| path_key(root) != canonical_project_key)
        .flat_map(|root| {
            let plan_id = plan_id.clone();
            ["active", "archive"]
                .into_iter()
                .map(move |bucket| {
                    root.join(".codeterm")
                        .join("plans")
                        .join(bucket)
                        .join(plan_stem(&plan_id))
                        .join("plan.yaml")
                })
        })
        .filter(|path| path.is_file())
        .map(|path| canonicalize_path(&path))
        .filter(|path| path_key(path) != canonical_plan_key)
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

pub(super) fn canonicalize_path(path: &Path) -> PathBuf {
    normalize_path(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

#[cfg(windows)]
fn normalize_path(path: PathBuf) -> PathBuf {
    PathBuf::from(strip_windows_verbatim_prefix(&path.display().to_string()))
}

#[cfg(not(windows))]
fn normalize_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn strip_windows_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

#[cfg(windows)]
fn path_key(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('/', r"\")
        .to_ascii_lowercase()
}

#[cfg(not(windows))]
fn path_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn plan_stem(id: &str) -> &str {
    if id == "inbox" {
        "_inbox"
    } else {
        id
    }
}

pub(super) fn inspect_plan(path: &Path, plan: &Value, kind: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    if matches!(kind, "all" | "storage") {
        findings.extend(inspect_storage(path, plan));
    }
    if matches!(kind, "all" | "structure") {
        findings.extend(inspect_structure(path, plan));
    }
    if matches!(kind, "all" | "quality") {
        findings.extend(super::quality::inspect_quality(path, plan));
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
    findings.extend(inspect_orphan_attachment_dirs(path, plan, tasks));
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

fn inspect_orphan_attachment_dirs(path: &Path, plan: &Value, tasks: &[Value]) -> Vec<Finding> {
    let Some(plan_dir) = path.parent() else {
        return vec![];
    };
    let root = plan_dir.join("attachments");
    if !root.is_dir() {
        return vec![];
    }
    let task_ids = tasks
        .iter()
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let milestone_ids = tasks
        .iter()
        .flat_map(|task| {
            task.get("milestones")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|milestone| {
            milestone
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<HashSet<_>>();
    let section_ids = collect_section_ids(plan.get("sections").and_then(Value::as_array));
    let mut findings = vec![];
    attachment_orphans(path, plan, &root.join("tasks"), &task_ids, &mut findings);
    attachment_orphans(
        path,
        plan,
        &root.join("sections"),
        &section_ids,
        &mut findings,
    );
    attachment_orphans(
        path,
        plan,
        &root.join("milestones"),
        &milestone_ids,
        &mut findings,
    );
    findings
}

fn attachment_orphans(
    plan_path: &Path,
    plan: &Value,
    dir: &Path,
    live_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().map(|ft| !ft.is_dir()).unwrap_or(true) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if live_ids.contains(&id) {
            continue;
        }
        findings.push(Finding {
            severity: "warning",
            kind: "orphan_attachment_dir",
            plan_id: plan_id(plan, plan_path),
            path: entry.path().display().to_string(),
            task_id: None,
            detail: format!("attachment directory has no matching live record id '{id}'"),
            fix: "delete the orphaned attachment directory after confirming the parent was intentionally removed".into(),
        });
    }
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
    findings.extend(inspect_group_regression_gates(path, plan, &tasks));
    findings
}

fn inspect_group_regression_gates(path: &Path, plan: &Value, tasks: &[Value]) -> Vec<Finding> {
    let mut by_group: HashMap<String, Vec<&Value>> = HashMap::new();
    for task in tasks {
        if let Some(key) = task.get("group").and_then(Value::as_str) {
            by_group.entry(key.to_string()).or_default().push(task);
        }
    }
    let mut keys: Vec<&String> = by_group.keys().collect();
    keys.sort();
    let mut findings = Vec::new();
    for key in keys {
        let members = &by_group[key];
        let all_terminal = members.iter().all(|t| {
            matches!(
                t.get("status").and_then(Value::as_str),
                Some("done") | Some("cancelled")
            )
        });
        let any_done = members
            .iter()
            .any(|t| t.get("status").and_then(Value::as_str) == Some("done"));
        if !all_terminal || !any_done {
            continue;
        }
        let last_close = members
            .iter()
            .filter_map(|t| t.get("status_changed_at").and_then(Value::as_u64))
            .max()
            .unwrap_or(0);
        if group_stride_ts(plan, key, REGRESSION_MILESTONE_KIND).is_none_or(|ts| ts < last_close) {
            findings.push(Finding {
                severity: "warning",
                kind: "group_missing_regression",
                plan_id: plan_id(plan, path),
                path: path.display().to_string(),
                task_id: None,
                detail: format!(
                    "group '{key}' is complete but has no full-gate regression run on record since its last close"
                ),
                fix: "run the full gate once and record it: codeterm plan qa next --group <group> --claim --note \"<scope>\"".into(),
            });
        }
        if group_stride_ts(plan, key, GROUP_CLOSE_MILESTONE_KIND).is_none_or(|ts| ts < last_close) {
            findings.push(Finding {
                severity: "warning",
                kind: "group_missing_retrospective",
                plan_id: plan_id(plan, path),
                path: path.display().to_string(),
                task_id: None,
                detail: format!(
                    "group '{key}' is complete but has no retrospective on record since its last close, so no further task dispatches"
                ),
                fix: format!("record it: codeterm plan groups close <plan> {key} --note \"<what worked, what failed, what changes next>\""),
            });
        }
    }
    findings
}

fn group_stride_ts(plan: &Value, key: &str, kind: &str) -> Option<u64> {
    let link = format!("{GROUP_REGRESSION_LINK_PREFIX}{key}");
    plan.get("strides")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("kind").and_then(Value::as_str) == Some(kind))
        .filter(|m| {
            m.get("links")
                .and_then(Value::as_array)
                .is_some_and(|links| links.iter().any(|l| l.as_str() == Some(link.as_str())))
        })
        .filter_map(|m| m.get("ts").and_then(Value::as_u64))
        .max()
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

#[cfg(all(test, windows))]
mod tests {
    use super::strip_windows_verbatim_prefix;

    #[test]
    fn strips_windows_verbatim_prefixes() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\C:\tmp\repo"),
            r"C:\tmp\repo"
        );
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\UNC\server\share\repo"),
            r"\\server\share\repo"
        );
    }
}
