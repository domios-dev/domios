use super::report::SharedReportError;
use super::{PlanError, PlanStore, TaskFiles, WorkerPlacement};
use crate::plan::Task;
use crate::service_plan::task_authority_projection::SharedReportRead as SharedReportSnapshot;

pub(crate) struct SharedDispatchInspection {
    pub checkout: String,
    pub head: String,
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
            files: task.files.clone(),
        })
    }
}

pub(crate) struct SharedReportInspection {
    snapshot: SharedReportSnapshot,
    changed_files: Vec<String>,
}

impl SharedReportInspection {
    pub fn recheck<'a>(
        task: &Task,
        reporter: &str,
        inspected: Option<&'a Self>,
    ) -> Result<Option<&'a [String]>, PlanError> {
        match (SharedReportSnapshot::capture(task, reporter), inspected) {
            (None, None) => Ok(None),
            (Some(current), Some(inspected)) if current == inspected.snapshot => {
                Ok(Some(&inspected.changed_files))
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
        self.observe_shared_history(&checkout, now_ms);
        let head = self
            .shared_git(&checkout, &["rev-parse", "--verify", "HEAD^{commit}"])
            .map_err(|error| PlanError::InvalidPlanText(error.to_string()))?
            .trim()
            .to_string();
        Ok(SharedDispatchInspection {
            checkout,
            head,
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
        }))
    }
}
