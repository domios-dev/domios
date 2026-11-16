use super::report::SharedReportError;
use super::{PlanError, PlanStore, TaskFiles, WorkerPlacement};
use crate::plan::Task;
use crate::service_plan::task_authority_projection::SharedReportRead as SharedReportSnapshot;

/// Where a shared attempt starts: a commit, or declared-file hashes in a
/// project folder without git history.
pub(crate) enum SharedStart {
    Commit(String),
    Files(super::FileHashes),
}

pub(crate) struct SharedDispatchInspection {
    pub checkout: String,
    pub start: SharedStart,
    files: TaskFiles,
}

impl SharedDispatchInspection {
    pub fn recheck(&self, checkout: &str, files: &TaskFiles) -> Result<(), PlanError> {
        if self.checkout != checkout || self.files != *files {
            return Err(PlanError::InvalidPlanText(
                "shared dispatch context changed during Git inspection; dispatch again".into(),
            ));
        }
        Ok(())
    }
}

impl SharedReportSnapshot {
    fn capture(task: &Task, reporter: &str) -> Option<Self> {
        // A non_repository task carries no Git attribution, whatever its attempt placement.
        if !task.execution_intent.requires_repository()
            || task.assigned_tab_id.as_deref() != Some(reporter)
        {
            return None;
        }
        let attempt = task.execution_attempts.iter().rfind(|attempt| {
            attempt.executor_tab_id == reporter
                && attempt.closed_at.is_none()
                && attempt.retired_at.is_none()
                && (task.interruption.owner_epoch == 0
                    || attempt.interruption.owner_epoch == task.interruption.owner_epoch)
        })?;
        if attempt.placement != WorkerPlacement::Shared {
            return None;
        }
        Some(Self {
            attempt_id: attempt.id.clone(),
            executor_tab_id: attempt.executor_tab_id.clone(),
            checkout: attempt.checkout_path.clone(),
            start_sha: attempt.shared_start_sha.clone(),
            start_files: attempt.shared_start_files.clone(),
            files: task.files.clone(),
        })
    }
}

pub(crate) struct SharedReportInspection {
    snapshot: SharedReportSnapshot,
    pub changed_files: Vec<String>,
    /// Empty for a git project; the hash evidence for a folder without git.
    pub changed_file_hashes: super::ChangedFileHashes,
}

impl SharedReportInspection {
    pub fn recheck<'a>(
        task: &Task,
        reporter: &str,
        inspected: Option<&'a Self>,
    ) -> Result<Option<&'a Self>, PlanError> {
        match (SharedReportSnapshot::capture(task, reporter), inspected) {
            (None, None) => Ok(None),
            (Some(current), Some(inspected)) if current == inspected.snapshot => {
                Ok(Some(inspected))
            }
            _ => Err(PlanError::InvalidPlanText(
                "shared report context changed during Git inspection; re-report".into(),
            )),
        }
    }
}

impl PlanStore {
    fn shared_git(&self, checkout: &str, args: &[&str]) -> Result<String, SharedReportError> {
        #[cfg(test)]
        {
            let observer = self.shared_git_observer.read().clone();
            if let Some(observer) = observer {
                observer(checkout, args);
            }
        }
        super::report::git(checkout, args)
    }

    pub(crate) fn inspect_shared_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        now_ms: u64,
    ) -> Result<SharedDispatchInspection, PlanError> {
        let read = self.database.shared_dispatch_projection(plan_id, task_id)?;
        read.files
            .shared_edit_paths()
            .map_err(PlanError::InvalidPlanText)?;
        super::validate_edit_links_at(&super::canonical_path(&read.checkout), &read.files)
            .map_err(PlanError::InvalidPlanText)?;
        let checkout = read.checkout;
        let start = match super::project_history(&checkout).map_err(PlanError::InvalidPlanText)? {
            super::ProjectHistory::NoGit => SharedStart::Files(
                super::plain_folder::hash_declared_files(&checkout, &read.files)
                    .map_err(PlanError::InvalidPlanText)?,
            ),
            super::ProjectHistory::Git => {
                self.observe_shared_history(&checkout, now_ms);
                SharedStart::Commit(
                    self.shared_git(&checkout, &["rev-parse", "--verify", "HEAD^{commit}"])
                        .map_err(|error| PlanError::InvalidPlanText(error.to_string()))?
                        .trim()
                        .to_string(),
                )
            }
        };
        Ok(SharedDispatchInspection {
            checkout,
            start,
            files: read.files,
        })
    }

    pub(crate) fn inspect_shared_report(
        &self,
        plan_id: &str,
        task_id: &str,
        reporter: &str,
        sha: Option<&str>,
        now_ms: u64,
    ) -> Result<Option<SharedReportInspection>, PlanError> {
        let Some(snapshot) = self
            .database
            .shared_report_projection(plan_id, task_id, reporter)?
        else {
            return Ok(None);
        };
        let checkout = snapshot.checkout.as_deref().ok_or_else(|| {
            PlanError::InvalidPlanText("shared attempt has no checkout binding".into())
        })?;
        if let Some(start) = snapshot.start_files.as_ref() {
            let current = super::plain_folder::hash_declared_files(checkout, &snapshot.files)
                .map_err(PlanError::InvalidPlanText)?;
            let changed_file_hashes = super::plain_folder::validate_report_without_git(
                &snapshot.files,
                start,
                &current,
                sha,
            )
            .map_err(|error| PlanError::SharedReportRejected(error.to_string()))?;
            return Ok(Some(SharedReportInspection {
                snapshot,
                changed_files: changed_file_hashes.keys().cloned().collect(),
                changed_file_hashes,
            }));
        }
        self.observe_shared_history(checkout, now_ms);
        let start = snapshot
            .start_sha
            .as_deref()
            .ok_or_else(|| PlanError::InvalidPlanText("shared attempt has no start SHA".into()))?;
        let changed_files = super::report::validate_shared_report_with_git(
            &snapshot.files,
            checkout,
            start,
            sha,
            |checkout, args| self.shared_git(checkout, args),
        )
        .map_err(|error| match error {
            SharedReportError::Rejected(cause) => PlanError::SharedReportRejected(cause),
            SharedReportError::Inspection(cause) => PlanError::InvalidPlanText(cause),
        })?;
        Ok(Some(SharedReportInspection {
            snapshot,
            changed_files,
            changed_file_hashes: Default::default(),
        }))
    }
}
