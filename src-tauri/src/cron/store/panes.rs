//! Pane-ownership tracking + per-job log persistence on the [`super::CronStore`].

use std::fs;
use std::path::PathBuf;

use super::CronStore;

impl CronStore {
    pub fn register_pane(&mut self, pane_id: &str, job_id: &str, run_id: &str) {
        self.pane_owners.insert(pane_id.to_string(), (job_id.to_string(), run_id.to_string()));
    }

    pub fn get_pane_owner(&self, pane_id: &str) -> Option<&(String, String)> {
        self.pane_owners.get(pane_id)
    }

    pub fn unregister_pane(&mut self, pane_id: &str) {
        self.pane_owners.remove(pane_id);
    }

    pub fn pane_owners_for_job(&self, job_id: &str) -> Vec<String> {
        self.pane_owners
            .iter()
            .filter(|(_, (jid, _))| jid == job_id)
            .map(|(pid, _)| pid.clone())
            .collect()
    }

    pub fn is_job_running(&self, job_id: &str) -> bool {
        self.pane_owners.values().any(|(jid, _)| jid == job_id)
    }

    pub fn get_pane_owner_by_run(&self, run_id: &str) -> Option<(&str, &str)> {
        for (pane_id, (job_id, rid)) in &self.pane_owners {
            if rid == run_id {
                return Some((job_id.as_str(), pane_id.as_str()));
            }
        }
        None
    }

    pub fn logs_dir(&self) -> PathBuf {
        let dir = self.file_path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("crons");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    pub fn save_log(&self, job_id: &str, run_id: &str, text: &str) -> Result<PathBuf, String> {
        let job_dir = self.logs_dir().join(job_id);
        let _ = fs::create_dir_all(&job_dir);
        let path = job_dir.join(format!("{}.log", run_id));
        fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn read_log(&self, job_id: &str, run_id: &str) -> Option<String> {
        let path = self.logs_dir().join(job_id).join(format!("{}.log", run_id));
        fs::read_to_string(path).ok()
    }
}
