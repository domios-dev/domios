use std::path::{Path, PathBuf};

use crate::service::plan::PlanError;
use crate::service::{team_core, ServiceContext};

impl ServiceContext {
    pub fn ensure_plan_project_loaded(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
        self.load_plan_project(project_path, true)
            .map(|(project_root, _)| project_root)
    }

    /// Startup restore loads every project, then reconciles team bindings once:
    /// each pass scans every plan, and loading a project adds none. Returns
    /// whether the project was newly loaded.
    pub fn load_plan_project_deferring_team_reconcile(
        &self,
        project_path: &Path,
    ) -> Result<bool, PlanError> {
        self.load_plan_project(project_path, false)
            .map(|(_, newly_loaded)| newly_loaded)
    }

    fn load_plan_project(
        &self,
        project_path: &Path,
        reconcile_team_bindings: bool,
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
        if should_scan && reconcile_team_bindings {
            team_core::reconcile_dangling_team_plan_bindings_now(self)?;
        }
        let legacy_plans_dir = project_root.join(".domios").join("plans");
        let mut watchers = self.ext::<crate::service::PlanWatchers>().0.lock();
        if legacy_plans_dir.is_dir() && !watchers.contains_key(&project_root) {
            match crate::service::plan::watcher::PlanWatcher::watch_project(
                &project_root,
                self.plan_store.clone(),
                std::sync::Arc::new(crate::service::plan::sink_impl::TauriPlanEventSink::new(
                    self.ui.clone(),
                )),
            ) {
                Ok(watcher) => {
                    watchers.insert(project_root.clone(), watcher);
                }
                Err(e) => log::warn!("[plan] failed to watch {}: {}", project_root.display(), e),
            }
        }
        Ok((project_root, should_scan))
    }
}
