use std::path::{Path, PathBuf};

pub(crate) fn project_root(project: Option<String>) -> Result<PathBuf, String> {
    let raw = project.unwrap_or_else(|| ".".into());
    PathBuf::from(raw)
        .canonicalize()
        .map_err(|e| format!("resolve project path: {e}"))
}

pub(crate) fn plan_files(plans_dir: &Path, plan_id: Option<&str>) -> Result<Vec<PathBuf>, String> {
    if let Some(id) = plan_id {
        return Ok(vec![plans_dir.join(format!("{id}.json"))]);
    }
    let mut files = Vec::new();
    let entries =
        std::fs::read_dir(plans_dir).map_err(|e| format!("read {}: {e}", plans_dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) fn plan_id_from(plan: &serde_json::Value, path: &Path) -> Option<String> {
    plan.get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
}

pub(crate) fn is_inbox_plan(plan: &serde_json::Value, path: &Path) -> bool {
    plan.get("id")
        .and_then(|v| v.as_str())
        .is_some_and(|id| id == "inbox")
        || path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem == "_inbox")
}
