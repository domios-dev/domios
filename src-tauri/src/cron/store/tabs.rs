//! Tab-ownership tracking + per-job log persistence on the [`super::CronStore`].

use std::fs;
use std::path::PathBuf;

use super::CronStore;

impl CronStore {
    pub fn register_tab(&mut self, tab_id: &str, job_id: &str, run_id: &str) {
        self.tab_owners
            .insert(tab_id.to_string(), (job_id.to_string(), run_id.to_string()));
    }

    pub fn get_tab_owner(&self, tab_id: &str) -> Option<&(String, String)> {
        self.tab_owners.get(tab_id)
    }

    pub fn unregister_tab(&mut self, tab_id: &str) {
        self.tab_owners.remove(tab_id);
    }

    pub fn tab_owners_for_job(&self, job_id: &str) -> Vec<String> {
        self.tab_owners
            .iter()
            .filter(|(_, (jid, _))| jid == job_id)
            .map(|(pid, _)| pid.clone())
            .collect()
    }

    pub fn is_job_running(&self, job_id: &str) -> bool {
        self.tab_owners.values().any(|(jid, _)| jid == job_id)
    }

    pub fn get_tab_owner_by_run(&self, run_id: &str) -> Option<(&str, &str)> {
        for (tab_id, (job_id, rid)) in &self.tab_owners {
            if rid == run_id {
                return Some((job_id.as_str(), tab_id.as_str()));
            }
        }
        None
    }

    pub fn set_tab_awaiting_owner_prompt(
        &mut self,
        tab_id: &str,
        awaiting: bool,
    ) -> Result<bool, String> {
        let Some((job_id, run_id)) = self.tab_owners.get(tab_id).cloned() else {
            return Ok(false);
        };
        let next = if awaiting {
            super::CronRunStatus::AwaitingOwnerPrompt
        } else {
            super::CronRunStatus::Running
        };
        let mut changed = false;
        if let Some(run) = self
            .file
            .history
            .get_mut(&job_id)
            .and_then(|history| history.iter_mut().find(|run| run.run_id == run_id))
        {
            if matches!(
                run.status,
                super::CronRunStatus::Running | super::CronRunStatus::AwaitingOwnerPrompt
            ) && run.status != next
            {
                run.status = next.clone();
                changed = true;
            }
        }
        if let Some(run) = self
            .file
            .jobs
            .iter_mut()
            .find(|job| job.id == job_id)
            .and_then(|job| job.last_run.as_mut())
            .filter(|run| run.run_id == run_id)
        {
            if matches!(
                run.status,
                super::CronRunStatus::Running | super::CronRunStatus::AwaitingOwnerPrompt
            ) && run.status != next
            {
                run.status = next;
                changed = true;
            }
        }
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn logs_dir(&self) -> PathBuf {
        let dir = self
            .file_path
            .parent()
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
