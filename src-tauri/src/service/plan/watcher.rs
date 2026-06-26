use super::PlanStore;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, Debouncer};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

pub struct PlanWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher>,
}

impl PlanWatcher {
    pub fn watch_project(
        project_path: &Path,
        store: Arc<PlanStore>,
        app: AppHandle,
    ) -> Result<Self, notify::Error> {
        let plans_dir = super::paths::plans_dir(project_path).map_err(notify::Error::io)?;
        let mut debouncer = new_debouncer(Duration::from_millis(200), move |events| {
            handle_events(events, &store, &app);
        })?;
        debouncer
            .watcher()
            .watch(&plans_dir, RecursiveMode::Recursive)?;
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

fn handle_events(
    events: notify_debouncer_mini::DebounceEventResult,
    store: &Arc<PlanStore>,
    app: &AppHandle,
) {
    let Ok(events) = events else {
        return;
    };
    for event in events {
        let path = event.path;
        if store.is_own_recent_write(&path) {
            continue;
        }
        let Some(plan_id) = plan_id_from_path(&path) else {
            continue;
        };
        let loaded = store.reload_from_disk(&plan_id).or_else(|_| {
            project_from_plan_path(&path)
                .ok_or(super::PlanError::NotFound)
                .and_then(|project| store.scan_project(&project).map(|_| ()))
        });
        if loaded.is_ok() {
            crate::api::events::emit_everywhere(
                app,
                "plan-changed",
                serde_json::json!({ "plan_id": plan_id }),
            );
        } else if let Err(e) = loaded {
            log::warn!("[plan] watcher reload failed for {}: {}", path.display(), e);
        }
    }
}

fn plan_id_from_path(path: &Path) -> Option<String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s == crate::plan::INBOX_PLAN_FILE_STEM {
                    crate::plan::INBOX_PLAN_ID.to_string()
                } else {
                    s.to_string()
                }
            }),
        Some("md") => plan_id_from_body_path(path),
        Some("jsonl") => plan_id_from_stride_log_path(path),
        _ => None,
    }
}

fn plan_id_from_body_path(path: &Path) -> Option<String> {
    let file_parent = path.parent()?;
    let kind = file_parent.file_name()?.to_str()?;
    if kind != "sections" && kind != "tasks" {
        return None;
    }
    file_parent
        .parent()?
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn plan_id_from_stride_log_path(path: &Path) -> Option<String> {
    if path.file_name()?.to_str()? != "strides.jsonl" {
        return None;
    }
    let task_dir = path.parent()?;
    let tasks_dir = task_dir.parent()?;
    if tasks_dir.file_name()?.to_str()? != "tasks" {
        return None;
    }
    tasks_dir
        .parent()?
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn project_from_plan_path(path: &Path) -> Option<std::path::PathBuf> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => path
            .parent()?
            .parent()?
            .parent()
            .map(std::path::Path::to_path_buf),
        Some("md") => path
            .parent()?
            .parent()?
            .parent()?
            .parent()?
            .parent()
            .map(std::path::Path::to_path_buf),
        Some("jsonl") if plan_id_from_stride_log_path(path).is_some() => path
            .parent()?
            .parent()?
            .parent()?
            .parent()?
            .parent()?
            .parent()
            .map(std::path::Path::to_path_buf),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_id_ignores_non_json_paths() {
        assert_eq!(
            plan_id_from_path(&std::path::PathBuf::from("/tmp/pl_1.json")),
            Some("pl_1".into())
        );
        assert_eq!(
            plan_id_from_path(&std::path::PathBuf::from("/tmp/pl_1.tmp")),
            None
        );
        assert_eq!(
            plan_id_from_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/sections/sec_1.md"
            )),
            Some("pl_1".into())
        );
        assert_eq!(
            plan_id_from_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/tasks/task_1/strides.jsonl"
            )),
            Some("pl_1".into())
        );
        assert_eq!(
            plan_id_from_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/tasks/task_1/other.jsonl"
            )),
            None
        );
        assert_eq!(
            plan_id_from_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/sections/sec_1/strides.jsonl"
            )),
            None
        );
    }

    #[test]
    fn project_from_plan_path_walks_out_of_codeterm_plans() {
        assert_eq!(
            project_from_plan_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1.json"
            )),
            Some(std::path::PathBuf::from("/tmp/project"))
        );
        assert_eq!(
            project_from_plan_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/tasks/task_1.md"
            )),
            Some(std::path::PathBuf::from("/tmp/project"))
        );
        assert_eq!(
            project_from_plan_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/tasks/task_1/strides.jsonl"
            )),
            Some(std::path::PathBuf::from("/tmp/project"))
        );
        assert_eq!(
            project_from_plan_path(&std::path::PathBuf::from(
                "/tmp/project/.codeterm/plans/pl_1/tasks/task_1/other.jsonl"
            )),
            None
        );
    }
}
