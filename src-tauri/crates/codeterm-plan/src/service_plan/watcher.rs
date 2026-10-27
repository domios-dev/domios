use super::ports::PlanEventSink;
use super::PlanStore;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, Debouncer};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub struct PlanWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher>,
}

impl PlanWatcher {
    pub fn watch_project(
        project_path: &Path,
        store: Arc<PlanStore>,
        sink: Arc<dyn PlanEventSink>,
    ) -> Result<Self, notify::Error> {
        let project_root = super::paths::project_root(project_path);
        let plans_dir = super::paths::plans_dir(&project_root).map_err(notify::Error::io)?;
        let mut debouncer = new_debouncer(Duration::from_millis(200), move |events| {
            handle_events(events, &project_root, &store, &sink);
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
    project_root: &Path,
    store: &Arc<PlanStore>,
    sink: &Arc<dyn PlanEventSink>,
) {
    let Ok(events) = events else {
        return;
    };
    let event_count = events.len();
    let plan_ids = coalesce_plan_ids(events.iter().map(|e| e.path.as_path()));
    if plan_ids.is_empty() {
        return;
    }
    log::debug!(
        "[plan] watcher burst: {} event(s) coalesced to {} plan(s)",
        event_count,
        plan_ids.len()
    );
    for plan_id in plan_ids {
        match store.reconcile_watched_plan(project_root, &plan_id) {
            Ok(outcome) => {
                if outcome.should_emit() {
                    log::info!("[plan] watcher reconciled {plan_id}: {outcome:?}");
                    sink.emit_plan_changed(&plan_id);
                }
            }
            Err(e) => log::warn!("[plan] watcher reconcile failed for {plan_id}: {e}"),
        }
    }
}

/// Collapse a debounced FS burst into the distinct set of affected plan ids.
fn coalesce_plan_ids<'a>(paths: impl Iterator<Item = &'a Path>) -> BTreeSet<String> {
    paths.filter_map(plan_id_from_path).collect()
}

fn plan_id_from_path(path: &Path) -> Option<String> {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    for i in 0..comps.len() {
        if comps[i] != "plans" || i + 2 >= comps.len() {
            continue;
        }
        if !matches!(comps[i + 1], "active" | "archive") {
            continue;
        }
        let stem = comps[i + 2];
        if stem.is_empty() {
            return None;
        }
        return Some(stem.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn plan_id_ignores_non_plan_paths() {
        assert_eq!(plan_id_from_path(Path::new("/tmp/pl_1.json")), None);
        assert_eq!(plan_id_from_path(Path::new("/tmp/pl_1.tmp")), None);
    }

    #[test]
    fn plan_id_from_plan_json_and_bodies() {
        assert_eq!(
            plan_id_from_path(Path::new(
                "/tmp/project/.domios/plans/active/pl_1/plan.json"
            )),
            Some("pl_1".into())
        );
        assert_eq!(
            plan_id_from_path(Path::new(
                "/tmp/project/.domios/plans/active/pl_1/sections/sec_1.md"
            )),
            Some("pl_1".into())
        );
        assert_eq!(
            plan_id_from_path(Path::new(
                "/tmp/project/.domios/plans/active/pl_1/tasks/task_1/strides.jsonl"
            )),
            Some("pl_1".into())
        );
    }

    #[test]
    fn plan_id_from_directory_delete_event() {
        // Whole-plan external deletes surface as a directory-level event, not a file.
        assert_eq!(
            plan_id_from_path(Path::new("/tmp/project/.domios/plans/active/pl_1")),
            Some("pl_1".into())
        );
        assert_eq!(
            plan_id_from_path(Path::new("/tmp/project/.domios/plans/archive/pl_2")),
            Some("pl_2".into())
        );
    }

    #[test]
    fn burst_coalesces_many_events_to_distinct_plans() {
        let base = "/tmp/project/.domios/plans/active";
        let paths = [
            format!("{base}/pl_1/plan.json"),
            format!("{base}/pl_1/sections/sec_1.md"),
            format!("{base}/pl_1/tasks/task_1/strides.jsonl"),
            format!("{base}/pl_2/plan.json"),
            "/tmp/project/.domios/plans/active".to_string(),
        ];
        let bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let ids = coalesce_plan_ids(bufs.iter().map(PathBuf::as_path));
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("pl_1"));
        assert!(ids.contains("pl_2"));
    }

    #[test]
    fn plan_id_none_above_stem() {
        assert_eq!(
            plan_id_from_path(Path::new("/tmp/project/.domios/plans/active")),
            None
        );
        assert_eq!(
            plan_id_from_path(Path::new("/tmp/project/.domios/plans")),
            None
        );
    }
}
