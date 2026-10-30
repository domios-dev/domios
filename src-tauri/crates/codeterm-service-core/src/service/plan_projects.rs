use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::service::plan::watcher::PlanWatcher;
use crate::service::plan::PlanError;
use crate::service::{team_core, ServiceContext};

impl ServiceContext {
    pub fn ensure_plan_project_loaded(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
        self.load_plan_project(project_path, false)
            .map(|(project_root, _)| project_root)
    }

    /// Startup restore loads every project, then reconciles team bindings once:
    /// each pass scans every plan, and loading a project adds none. The legacy
    /// plan-folder watcher starts on the blocking pool. Returns whether the
    /// project was newly loaded.
    pub fn load_plan_project_deferring_team_reconcile(
        &self,
        project_path: &Path,
    ) -> Result<bool, PlanError> {
        self.load_plan_project(project_path, true)
            .map(|(_, newly_loaded)| newly_loaded)
    }

    fn load_plan_project(
        &self,
        project_path: &Path,
        startup: bool,
    ) -> Result<(PathBuf, bool), PlanError> {
        let project_root = crate::service::plan::paths::project_root(project_path);
        let should_scan = {
            let loaded = self.ext::<crate::service::LoadedPlanProjects>().0.lock();
            !loaded.contains(&project_root)
        };
        if should_scan {
            self.plan_store.scan_project(&project_root)?;
            self.ext::<crate::service::LoadedPlanProjects>()
                .0
                .lock()
                .insert(project_root.clone());
        }
        if should_scan && !startup {
            team_core::reconcile_dangling_team_plan_bindings_now(self)?;
        }
        if project_root.join(".domios").join("plans").is_dir() {
            let watch = PlanFolderWatch {
                watchers: self.ext::<crate::service::PlanWatchers>().0.clone(),
                store: self.plan_store.clone(),
                ui: self.ui.clone(),
                project_root: project_root.clone(),
            };
            if startup {
                // A stream start can take seconds while catalogue watchers
                // install at boot; restore never waits for one.
                crate::async_runtime::spawn_blocking(move || watch.start());
            } else {
                watch.start();
            }
        }
        Ok((project_root, should_scan))
    }
}

/// Watches a project's legacy plan folder; the registry lock is never held
/// while the stream starts, and a duplicate start is dropped outside it.
struct PlanFolderWatch {
    watchers: Arc<Mutex<HashMap<PathBuf, PlanWatcher>>>,
    store: Arc<crate::service::plan::PlanStore>,
    ui: Arc<dyn crate::ports::host::UiEvents>,
    project_root: PathBuf,
}

impl PlanFolderWatch {
    fn start(self) {
        if self.watchers.lock().contains_key(&self.project_root) {
            return;
        }
        let started = std::time::Instant::now();
        let sink = Arc::new(crate::service::plan::sink_impl::TauriPlanEventSink::new(
            self.ui,
        ));
        match PlanWatcher::watch_project(&self.project_root, self.store, sink) {
            Ok(watcher) => {
                let duplicate = {
                    let mut watchers = self.watchers.lock();
                    if watchers.contains_key(&self.project_root) {
                        Some(watcher)
                    } else {
                        watchers.insert(self.project_root.clone(), watcher);
                        None
                    }
                };
                drop(duplicate);
            }
            Err(e) => log::warn!(
                "[plan] failed to watch {}: {}",
                self.project_root.display(),
                e
            ),
        }
        log::debug!(
            "[boot-operation] plan folder watch project={} watch_ms={}",
            self.project_root.display(),
            started.elapsed().as_millis()
        );
    }
}
