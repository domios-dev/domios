use super::{Plan, PlanError, PlanStore, Task, TaskFiles, WorkerPlacement};

fn checkout_path(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    }
}

pub(crate) struct SharedDispatchInspection {
    pub checkout: String,
    pub head: String,
    files: TaskFiles,
}

impl SharedDispatchInspection {
    pub fn recheck(&self, plan: &Plan, task: &Task) -> Result<(), PlanError> {
        if self.checkout != checkout_path(plan) || self.files != task.files {
            return Err(PlanError::InvalidPlanText(
                "shared dispatch context changed during Git inspection; dispatch again".into(),
            ));
        }
        Ok(())
    }
}

#[derive(PartialEq, Eq)]
struct SharedReportSnapshot {
    attempt_id: String,
    executor_tab_id: String,
    checkout: Option<String>,
    start_sha: Option<String>,
    files: TaskFiles,
}

impl SharedReportSnapshot {
    fn capture(task: &Task, reporter: &str) -> Option<Self> {
        if task.assigned_tab_id.as_deref() != Some(reporter) {
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
    fn shared_git(&self, checkout: &str, args: &[&str]) -> Result<String, String> {
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
    ) -> Result<SharedDispatchInspection, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        task.files
            .shared_edit_paths()
            .map_err(PlanError::InvalidPlanText)?;
        super::validate_edit_links(&plan, &task.files).map_err(PlanError::InvalidPlanText)?;
        let checkout = checkout_path(&plan).to_string();
        let head = self
            .shared_git(&checkout, &["rev-parse", "--verify", "HEAD^{commit}"])
            .map_err(PlanError::InvalidPlanText)?
            .trim()
            .to_string();
        Ok(SharedDispatchInspection {
            checkout,
            head,
            files: task.files.clone(),
        })
    }

    pub(crate) fn inspect_shared_report(
        &self,
        plan_id: &str,
        task_id: &str,
        reporter: &str,
        sha: Option<&str>,
    ) -> Result<Option<SharedReportInspection>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        let Some(snapshot) = SharedReportSnapshot::capture(task, reporter) else {
            return Ok(None);
        };
        let checkout = snapshot.checkout.as_deref().ok_or_else(|| {
            PlanError::InvalidPlanText("shared attempt has no checkout binding".into())
        })?;
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
        .map_err(PlanError::InvalidPlanText)?;
        Ok(Some(SharedReportInspection {
            snapshot,
            changed_files,
        }))
    }
}
