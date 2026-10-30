use std::path::{Path, PathBuf};

use serde_json::Value;

pub(super) fn resolve_source_plan(from: &str, plan_id: &str) -> Result<String, String> {
    let root = PathBuf::from(from)
        .canonicalize()
        .map_err(|e| format!("resolve source path: {e}"))?;
    let candidates = source_plan_candidates(&root, plan_id);
    candidates
        .into_iter()
        .find(|path| path.join("plan.yaml").is_file() || path.join("plan.json").is_file())
        .map(|path| path.display().to_string())
        .ok_or_else(|| format!("plan {plan_id} not found under {}", root.display()))
}

fn source_plan_candidates(root: &Path, plan_id: &str) -> Vec<PathBuf> {
    let plans_root = if root.ends_with(".domios/plans") || root.ends_with("plans") {
        root.to_path_buf()
    } else if root.join(".domios").join("plans").is_dir() {
        root.join(".domios").join("plans")
    } else if root.join("plans").is_dir() {
        root.join("plans")
    } else {
        root.to_path_buf()
    };
    ["active", "archive"]
        .into_iter()
        .map(|bucket| plans_root.join(bucket).join(plan_id))
        .collect()
}

pub(super) fn render_adopt_report(value: &Value) -> String {
    let verb = if value.get("moved").and_then(Value::as_bool) == Some(true) {
        "Moved"
    } else if value.get("dry_run").and_then(Value::as_bool) == Some(true) {
        "Would adopt"
    } else {
        "Adopted"
    };
    let plan_id = value.get("plan_id").and_then(Value::as_str).unwrap_or("?");
    let target = value
        .get("target_project")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let tasks = value.get("task_count").and_then(Value::as_u64).unwrap_or(0);
    let mut out = format!("{verb} plan {plan_id} into {target} ({tasks} tasks)\n");
    if let Some(warnings) = value.get("warnings").and_then(Value::as_array) {
        for warning in warnings.iter().filter_map(Value::as_str) {
            out.push_str("warning: ");
            out.push_str(warning);
            out.push('\n');
        }
    }
    out
}

pub(super) fn render_plan_search(value: &Value) -> String {
    if !crate::cmd::output::verbose() {
        return crate::cmd::output::compact_list(value, None);
    }
    render_plan_search_verbose(value)
}

fn render_plan_search_verbose(value: &Value) -> String {
    let hits = value
        .get("hits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if hits.is_empty() {
        return "No plan search hits.\n".into();
    }
    let mut out = String::new();
    for hit in hits {
        let kind = hit.get("kind").and_then(|v| v.as_str()).unwrap_or("unit");
        let status = hit.get("status").and_then(|v| v.as_str()).unwrap_or("-");
        let title = hit.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let unit = hit.get("unit_id").and_then(|v| v.as_str()).unwrap_or("");
        let body = hit.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let preview = body
            .split_whitespace()
            .take(18)
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!("- {kind} [{status}] {title} ({unit})\n"));
        if !preview.is_empty() {
            out.push_str(&format!("  {preview}\n"));
        }
    }
    out
}
