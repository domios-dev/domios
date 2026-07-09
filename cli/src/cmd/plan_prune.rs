use std::path::Path;

use serde_json::{json, Value};

use crate::cmd::plan_local::{is_inbox_plan, plan_files, plan_id_from, project_root};

#[derive(Debug, Clone)]
struct Candidate {
    plan_id: String,
    path: String,
    title: String,
    reason: &'static str,
}

impl Candidate {
    fn json(&self) -> Value {
        json!({
            "plan_id": self.plan_id,
            "path": self.path,
            "title": self.title,
            "reason": self.reason,
        })
    }
}

pub fn run(json_mode: bool, apply: bool, project: Option<String>) -> Result<Option<Value>, String> {
    let report = prune_report(project, apply)?;
    if json_mode {
        return Ok(Some(report));
    }
    print!("{}", render_prune(&report));
    Ok(None)
}

fn prune_report(project: Option<String>, apply: bool) -> Result<Value, String> {
    let project_root = project_root(project)?;
    let plans_dir = project_root.join(".codeterm").join("plans");
    let files = plan_files(&plans_dir, None)?;
    let mut candidates = Vec::new();
    for path in files {
        if is_inbox_plan(&Value::Null, &path) {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let plan: Value = match serde_json::from_str(&raw) {
            Ok(plan) => plan,
            Err(_) => continue,
        };
        if is_inbox_plan(&plan, &path) {
            continue;
        }
        if let Some(reason) = prune_reason(&plan) {
            candidates.push(Candidate {
                plan_id: plan_id_from(&plan, &path).unwrap_or_else(|| "?".into()),
                path: path.display().to_string(),
                title: plan
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
                reason,
            });
        }
    }
    let mut deleted = Vec::new();
    if apply {
        for candidate in &candidates {
            match delete_plan_file(&plans_dir, &candidate.plan_id, &candidate.path) {
                Ok(()) => deleted.push(candidate.json()),
                Err(err) => {
                    return Err(format!("delete {}: {err}", candidate.path));
                }
            }
        }
    }
    Ok(json!({
        "dry_run": !apply,
        "project": project_root.display().to_string(),
        "candidate_count": candidates.len(),
        "candidates": candidates.iter().map(Candidate::json).collect::<Vec<_>>(),
        "deleted": deleted,
    }))
}

fn prune_reason(plan: &Value) -> Option<&'static str> {
    if !is_effectively_empty(plan) {
        return None;
    }
    let title = plan.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if is_smoke_title(title) {
        return Some("smoke_empty");
    }
    if title.trim().is_empty() {
        return Some("empty_title");
    }
    None
}

fn is_effectively_empty(plan: &Value) -> bool {
    is_empty_array(plan.get("tasks"))
        && is_empty_array(plan.get("sections"))
        && is_empty_array(plan.get("elements"))
        && is_empty_array(plan.get("loops"))
        && is_empty_array(plan.get("spec_refs"))
        && is_empty_array(plan.get("spec_links"))
        && plan.get("goal").is_none_or(|v| v.is_null())
}

fn is_empty_array(value: Option<&Value>) -> bool {
    value.is_none_or(|v| v.as_array().is_none_or(|items| items.is_empty()))
}

fn is_smoke_title(title: &str) -> bool {
    let trimmed = title.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("smoke")
        || trimmed == "T"
        || lower == "test"
        || lower.starts_with("test ")
        || lower.starts_with("tmp ")
        || lower.starts_with("temp ")
}

fn delete_plan_file(plans_dir: &Path, plan_id: &str, json_path: &str) -> Result<(), String> {
    let path = Path::new(json_path);
    if !path.starts_with(plans_dir) {
        return Err("refusing to delete plan outside project plans dir".into());
    }
    let dir = path
        .parent()
        .ok_or_else(|| "plan JSON path has no parent directory".to_string())?;
    if dir.file_name().and_then(|s| s.to_str()) != Some(plan_stem(plan_id)) {
        return Err("plan JSON path does not match plan id".into());
    }
    if dir.is_dir() {
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn plan_stem(id: &str) -> &str {
    if id == "inbox" {
        "_inbox"
    } else {
        id
    }
}

pub fn render_prune(report: &Value) -> String {
    let dry_run = report
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let count = report
        .get("candidate_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mode = if dry_run { "dry-run" } else { "apply" };
    let mut out = format!("Plan prune ({mode}): {count} candidate(s)\n");
    let candidates = report
        .get("candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for candidate in candidates {
        let plan_id = candidate
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let reason = candidate
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let title = candidate
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        out.push_str(&format!("  - {plan_id} {reason}: \"{title}\"\n"));
    }
    if dry_run && count > 0 {
        out.push_str("Run with --apply to delete these plan files.\n");
    }
    let deleted = report
        .get("deleted")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !deleted.is_empty() {
        out.push_str(&format!("Deleted: {}\n", deleted.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn smoke_empty_plan_is_candidate() {
        let plan = json!({
            "id": "pl_smoke",
            "title": "geometry smoke",
            "status": "active",
            "tasks": [],
            "sections": []
        });
        assert_eq!(prune_reason(&plan), Some("smoke_empty"));
    }

    #[test]
    fn populated_plan_is_not_candidate() {
        let plan = json!({
            "id": "pl_1",
            "title": "smoke",
            "status": "active",
            "tasks": [{"id": "task_1"}],
            "sections": []
        });
        assert_eq!(prune_reason(&plan), None);
    }

    #[test]
    fn dry_run_does_not_delete() {
        let dir = tempfile::tempdir().unwrap();
        let plan_dir = dir.path().join(".codeterm/plans/active/pl_tmp");
        fs::create_dir_all(&plan_dir).unwrap();
        let plan = json!({
            "id": "pl_tmp",
            "title": "smoke test",
            "status": "active",
            "tasks": [],
            "sections": []
        });
        let path = plan_dir.join("plan.json");
        fs::write(&path, plan.to_string()).unwrap();
        let report = prune_report(Some(dir.path().display().to_string()), false).unwrap();
        assert_eq!(report["candidate_count"], 1);
        assert!(report["dry_run"].as_bool().unwrap());
        assert!(path.exists());
    }

    #[test]
    fn apply_deletes_smoke_plan() {
        let dir = tempfile::tempdir().unwrap();
        let plan_dir = dir.path().join(".codeterm/plans/active/pl_tmp");
        fs::create_dir_all(&plan_dir).unwrap();
        let plan = json!({
            "id": "pl_tmp",
            "title": "smoke test",
            "status": "active",
            "tasks": [],
            "sections": []
        });
        let path = plan_dir.join("plan.json");
        fs::write(&path, plan.to_string()).unwrap();
        let report = prune_report(Some(dir.path().display().to_string()), true).unwrap();
        assert_eq!(report["deleted"].as_array().unwrap().len(), 1);
        assert!(!path.exists());
    }
}
