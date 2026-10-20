//! Tab-ownership tracking + per-job log persistence on the [`super::CronStore`].
//!
//! Tab ownership is ephemeral process state, never persisted; run logs stay
//! append-only file assets at `crons/<job_id>/<run_id>.log`, never SQL bodies.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use super::{CronRunStatus, CronStore, Prepared};

/// One plain path component: no separator, no `.`/`..`, no prefix or root.
fn log_component(id: &str) -> Option<&str> {
    let mut components = Path::new(id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(part)), None)
            if part == id && !id.contains(|c: char| c == '/' || c == '\\') =>
        {
            Some(id)
        }
        _ => None,
    }
}

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

    /// Moves the run a tab owns between Running and AwaitingOwnerPrompt; returns whether
    /// anything changed.
    pub fn prepare_owner_prompt(&self, tab_id: &str, awaiting: bool) -> Prepared<bool> {
        let Some((job_id, run_id)) = self.tab_owners.get(tab_id) else {
            return Prepared::unchanged(false);
        };
        let next = if awaiting {
            CronRunStatus::AwaitingOwnerPrompt
        } else {
            CronRunStatus::Running
        };
        let mut edit = self.edit_run(job_id, run_id);
        let mut changed = false;
        edit.each(|run| {
            if matches!(
                run.status,
                CronRunStatus::Running | CronRunStatus::AwaitingOwnerPrompt
            ) && run.status != next
            {
                run.status = next.clone();
                changed = true;
            }
        });
        edit.into_prepared(self, changed)
    }

    pub fn set_tab_awaiting_owner_prompt(
        &mut self,
        tab_id: &str,
        awaiting: bool,
    ) -> Result<bool, String> {
        let prepared = self.prepare_owner_prompt(tab_id, awaiting);
        self.legacy(Ok(prepared))
    }

    pub fn logs_dir(&self) -> PathBuf {
        let _ = fs::create_dir_all(&self.logs_root);
        self.logs_root.clone()
    }

    /// The job's log directory, refused for an id that is not one plain path component.
    pub fn job_logs_dir(&self, job_id: &str) -> Option<PathBuf> {
        log_component(job_id).map(|job_id| self.logs_root.join(job_id))
    }

    fn log_path(&self, job_id: &str, run_id: &str) -> Option<PathBuf> {
        let file = format!("{}.log", log_component(run_id)?);
        self.job_logs_dir(job_id).map(|dir| dir.join(file))
    }

    /// Where a run's log lives, so callers can write it after releasing the cron lock.
    pub fn log_file(&self, job_id: &str, run_id: &str) -> Result<PathBuf, String> {
        self.log_path(job_id, run_id)
            .ok_or_else(|| format!("cron log id is not a plain name: {job_id}/{run_id}"))
    }

    pub fn save_log(&self, job_id: &str, run_id: &str, text: &str) -> Result<PathBuf, String> {
        let path = self.log_file(job_id, run_id)?;
        write_log(&path, text)?;
        Ok(path)
    }

    pub fn read_log(&self, job_id: &str, run_id: &str) -> Option<String> {
        fs::read_to_string(self.log_path(job_id, run_id)?).ok()
    }

    /// Publishes a complete log only where none exists; `Ok(false)` leaves the existing one
    /// byte-for-byte untouched, and a concurrent `write_log` still replaces what this created.
    pub fn create_log(path: &Path, text: &str) -> Result<bool, String> {
        let dir = path
            .parent()
            .ok_or_else(|| format!("cron log has no directory: {}", path.display()))?;
        let _ = fs::create_dir_all(dir);
        let mut staged = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        staged
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        match staged.persist_noclobber(path) {
            Ok(_) => Ok(true),
            Err(e) if e.error.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e.error.to_string()),
        }
    }
}

pub fn write_log(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    fs::write(path, text).map_err(|e| e.to_string())
}
