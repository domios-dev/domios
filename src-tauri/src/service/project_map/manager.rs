use super::eligibility::{evaluate, Verdict};
use super::runner::{resolve_cli_path, IndexProgress, RunCoordinator, RunSummary, Runner};
use super::watcher::{start_watcher, WorkspaceWatcher};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct ProjectMapManager {
    inner: Arc<Mutex<Inner>>,
    runner: Arc<Runner>,
}

struct Inner {
    workspaces: HashMap<String, WorkspaceSlot>,
}

struct WorkspaceSlot {
    name: String,
    cwd: PathBuf,
    watcher: Option<WorkspaceWatcher>,
    coord: Arc<RunCoordinator>,
    last_run: Option<RunSummary>,
    progress: Arc<Mutex<IndexProgress>>,
}

pub struct WorkspaceMeta<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub cwd: &'a Path,
    pub protected: bool,
}

impl ProjectMapManager {
    pub fn new(data_dir: &Path, socket_path: String) -> Arc<Self> {
        let cli_path = resolve_cli_path(data_dir);
        let runner = Arc::new(Runner::new(cli_path, socket_path));
        Arc::new(Self {
            inner: Arc::new(Mutex::new(Inner { workspaces: HashMap::new() })),
            runner,
        })
    }

    /// Bootstrap from existing workspaces. For each eligible one, spawn the
    /// initial index in the background and arm a watcher. Idempotent — calling
    /// it twice for the same workspace is a no-op (the second call sees the
    /// existing slot and returns early).
    pub fn bootstrap(self: &Arc<Self>, workspaces: &[(String, String, Option<String>, bool)]) {
        for (id, name, cwd, protected) in workspaces {
            if let Some(cwd) = cwd.as_deref() {
                self.start_for_workspace(WorkspaceMeta {
                    id,
                    name,
                    cwd: Path::new(cwd),
                    protected: *protected,
                });
            }
        }
    }

    pub fn start_for_workspace(self: &Arc<Self>, meta: WorkspaceMeta<'_>) {
        let WorkspaceMeta { id: ws_id, name, cwd, protected } = meta;
        let cfg = crate::service::project_map::config::read_config(cwd);
        let verdict = evaluate(protected, Some(cwd), cfg.indexing_enabled);
        if !verdict.is_eligible() {
            log::debug!(
                "[project-map] skip '{}' ({}): {:?}",
                name, ws_id, verdict
            );
            return;
        }

        {
            let mut inner = self.inner.lock();
            if inner.workspaces.contains_key(ws_id) {
                return;
            }
            let slot = WorkspaceSlot {
                name: name.to_string(),
                cwd: cwd.to_path_buf(),
                watcher: None,
                coord: Arc::new(RunCoordinator::new()),
                last_run: None,
                progress: Arc::new(Mutex::new(IndexProgress::default())),
            };
            inner.workspaces.insert(ws_id.to_string(), slot);
        }

        self.trigger_index(ws_id, false);
        self.arm_watcher(ws_id, cwd, Duration::from_millis(cfg.watch_debounce_ms));
    }

    pub fn stop_for_workspace(&self, ws_id: &str) {
        let mut inner = self.inner.lock();
        if inner.workspaces.remove(ws_id).is_some() {
            log::debug!("[project-map] stopped watcher for {}", ws_id);
        }
    }

    pub fn trigger_index(self: &Arc<Self>, ws_id: &str, force: bool) {
        let (coord, progress) = {
            let inner = self.inner.lock();
            match inner.workspaces.get(ws_id) {
                Some(slot) => (slot.coord.clone(), slot.progress.clone()),
                None => return,
            }
        };
        if !coord.try_acquire() {
            return;
        }

        let runner = self.runner.clone();
        let inner = self.inner.clone();
        let ws_id = ws_id.to_string();
        let manager = self.clone();
        std::thread::Builder::new()
            .name(format!("project-map-{}", ws_id))
            .spawn(move || loop {
                let summary = runner.run_index(&ws_id, force, progress.clone());
                if !summary.ok {
                    log::warn!("[project-map] {} failed: {}", ws_id, summary.message);
                } else {
                    log::debug!(
                        "[project-map] {} ok in {}ms",
                        ws_id, summary.elapsed_ms
                    );
                }
                {
                    let mut guard = inner.lock();
                    if let Some(slot) = guard.workspaces.get_mut(&ws_id) {
                        slot.last_run = Some(summary);
                    }
                }
                let again = coord.release();
                if !again {
                    break;
                }
                let still_present = manager.inner.lock().workspaces.contains_key(&ws_id);
                if !still_present {
                    break;
                }
                if !coord.try_acquire() {
                    break;
                }
            })
            .ok();
    }

    fn arm_watcher(self: &Arc<Self>, ws_id: &str, cwd: &Path, debounce: Duration) {
        let manager = self.clone();
        let ws_id_owned = ws_id.to_string();
        let watcher = start_watcher(cwd, debounce, move || {
            manager.trigger_index(&ws_id_owned, false);
        });
        match watcher {
            Ok(w) => {
                let mut inner = self.inner.lock();
                if let Some(slot) = inner.workspaces.get_mut(ws_id) {
                    slot.watcher = Some(w);
                }
            }
            Err(e) => log::warn!("[project-map] watch {}: {}", cwd.display(), e),
        }
    }

    pub fn last_run(&self, ws_id: &str) -> Option<RunSummary> {
        self.inner.lock().workspaces.get(ws_id).and_then(|s| s.last_run.clone())
    }

    pub fn progress(&self, ws_id: &str) -> Option<IndexProgress> {
        self.inner.lock().workspaces.get(ws_id).map(|s| s.progress.lock().clone())
    }

    /// Snapshot of currently watched workspaces, for debugging / status APIs.
    pub fn watched(&self) -> Vec<(String, String, PathBuf)> {
        self.inner
            .lock()
            .workspaces
            .iter()
            .map(|(id, slot)| (id.clone(), slot.name.clone(), slot.cwd.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> Arc<ProjectMapManager> {
        let tmp = tempfile::tempdir().unwrap();
        ProjectMapManager::new(tmp.path(), "unused-socket-for-test".into())
    }

    fn meta<'a>(ws_id: &'a str, name: &'a str, cwd: &'a Path, protected: bool) -> WorkspaceMeta<'a> {
        WorkspaceMeta { id: ws_id, name, cwd, protected }
    }

    #[test]
    fn skips_protected_workspace() {
        let m = make_manager();
        let tmp = tempfile::tempdir().unwrap();
        m.start_for_workspace(meta("ws-1", "Renamed", tmp.path(), true));
        assert!(m.watched().is_empty());
    }

    #[test]
    fn skips_when_disabled_in_config() {
        let m = make_manager();
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = crate::service::project_map::config::default_config();
        cfg.indexing_enabled = false;
        crate::service::project_map::config::write_config(tmp.path(), &cfg).unwrap();
        m.start_for_workspace(meta("ws-1", "proj", tmp.path(), false));
        assert!(m.watched().is_empty());
    }

    #[test]
    fn registers_eligible_workspace() {
        let m = make_manager();
        let tmp = tempfile::tempdir().unwrap();
        m.start_for_workspace(meta("ws-1", "proj", tmp.path(), false));
        let watched = m.watched();
        assert_eq!(watched.len(), 1);
        assert_eq!(watched[0].0, "ws-1");
        m.stop_for_workspace("ws-1");
        assert!(m.watched().is_empty());
    }
}
