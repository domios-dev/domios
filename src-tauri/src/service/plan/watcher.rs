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
            .watch(&plans_dir, RecursiveMode::NonRecursive)?;
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
        if store.reload_from_disk(&plan_id).is_ok() {
            crate::api::events::emit_everywhere(
                app,
                "plan-changed",
                serde_json::json!({ "plan_id": plan_id }),
            );
        }
    }
}

fn plan_id_from_path(path: &Path) -> Option<String> {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return None;
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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
    }
}
