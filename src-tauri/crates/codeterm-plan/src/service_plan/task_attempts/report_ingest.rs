use super::{BoundWorkerReport, PlanError, PlanStore};
use crate::plan::{AgentRef, Milestone, TaskExecutionReportSnapshot, TaskStatus};
use codeterm_types::agent::ReportStatus;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundWorkerReportReceipt {
    pub committed_review: bool,
    pub milestone_id: Option<String>,
}

impl PlanStore {
    pub fn record_task_attempt_report(
        &self,
        plan_id: &str,
        task_id: &str,
        from: &AgentRef,
        message: &str,
        status: Option<ReportStatus>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let Some(reporter_tab) = from.tab_id.as_deref() else {
            return Ok(());
        };
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Attempts,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                // Reports attach to the assigned executor's live attempt.
                // They never create or rebind an attempt.
                let executor_tab = task.assigned_tab_id.clone();
                let target_idx = executor_tab
                    .as_deref()
                    .and_then(|executor| {
                        task.execution_attempts.iter().rposition(|attempt| {
                            attempt.executor_tab_id == executor && attempt.closed_at.is_none()
                        })
                    })
                    .or_else(|| {
                        task.execution_attempts
                            .iter()
                            .rposition(|attempt| attempt.executor_tab_id == reporter_tab)
                    });
                if let Some(attempt) = target_idx.map(|idx| &mut task.execution_attempts[idx]) {
                    let reporter_tab_id =
                        (attempt.executor_tab_id != reporter_tab).then(|| reporter_tab.to_string());
                    attempt.report = Some(TaskExecutionReportSnapshot {
                        id: format!("report_{now_ms}"),
                        message: Some(message.into()),
                        status: status.map(super::report_status_key),
                        reporter_tab_id,
                        plan_skill_manifest: attempt.plan_skill_manifest.clone(),
                        loaded_plan_skills: attempt.loaded_plan_skills.clone(),
                        ..Default::default()
                    });
                    attempt.reported_at = Some(now_ms);
                    attempt.report_target_written_at = None;
                    if attempt.remote_dispatch_state.is_some() {
                        attempt.remote_dispatch_state =
                            Some(crate::plan::RemoteDispatchState::Reported);
                    }
                }
                Ok(())
            },
        )
    }

    /// Ingest a worker report for its currently assigned task in one plan write.
    /// Accepted report status and task state are changed in the same plan write.
    pub fn ingest_bound_worker_report(
        &self,
        plan_id: &str,
        task_id: &str,
        from: &AgentRef,
        mut report: BoundWorkerReport,
        now_ms: u64,
    ) -> Result<BoundWorkerReportReceipt, PlanError> {
        let Some(reporter_tab) = from.tab_id.as_deref() else {
            return Ok(BoundWorkerReportReceipt::default());
        };
        let transition = report
            .status
            .map(ReportStatus::task_transition)
            .transpose()
            .map_err(|error| PlanError::Io(error.into()))?
            .flatten();
        let shared_inspection = if transition == Some(TaskStatus::InReview) {
            self.inspect_shared_report(
                plan_id,
                task_id,
                reporter_tab,
                report.sha.as_deref(),
                now_ms,
            )?
        } else {
            None
        };
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Attempts,
            now_ms,
            |edit| {
                if edit.task.assigned_tab_id.as_deref() != Some(reporter_tab) {
                    return Ok(BoundWorkerReportReceipt::default());
                }
                crate::service_plan::task_attempt_gates::validate_authorized_gate_claim(
                    &edit.task,
                    reporter_tab,
                    report.test_cmd.as_deref(),
                )?;
                let interruption = &edit.task.interruption;
                if interruption.handoff_required
                    || (interruption.owner_epoch > 0
                        && interruption.owner_tab_id.as_deref() != Some(reporter_tab))
                {
                    return Ok(BoundWorkerReportReceipt::default());
                }
                if interruption.owner_epoch > 0 {
                    let authority = report.authority.as_ref().ok_or_else(|| {
                        PlanError::Io("report caller authority is missing".into())
                    })?;
                    crate::service_plan::interruption::authorize_task_mutation_for_owner(
                        edit.owner_tab.as_deref(),
                        &edit.task,
                        authority,
                    )?;
                    if interruption.owner_tab_id.as_deref() == Some(&authority.tab_id) {
                        let exact_attempt = edit.task.execution_attempts.iter().any(|attempt| {
                            attempt.executor_tab_id == authority.tab_id
                                && attempt.interruption.owner_epoch == authority.owner_epoch
                                && attempt.closed_at.is_none()
                        });
                        if !exact_attempt {
                            return Err(PlanError::Io("report attempt authority changed".into()));
                        }
                    }
                }
                if edit
                    .task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| attempt.executor_tab_id == reporter_tab)
                    .filter_map(|attempt| attempt.reported_at)
                    .any(|reported_at| reported_at >= now_ms)
                {
                    return Ok(BoundWorkerReportReceipt::default());
                }
                let current = edit.task.status.clone();
                if matches!(current, TaskStatus::Done | TaskStatus::Cancelled) {
                    return Ok(BoundWorkerReportReceipt::default());
                }
                let accepted_transition = if report.status == Some(ReportStatus::Blocked) {
                    None
                } else {
                    transition
                };
                let committed_review = accepted_transition == Some(TaskStatus::InReview);
                if committed_review {
                    if let Some(paths) =
                        super::super::shared_checkout::SharedReportInspection::recheck(
                            &edit.task,
                            reporter_tab,
                            shared_inspection.as_ref(),
                        )?
                    {
                        report.changed_files = paths.to_vec();
                    }
                }
                if let Some(next) = accepted_transition
                    .as_ref()
                    .filter(|next| *next != &current)
                {
                    crate::service_plan::mutations::status_transition::validate(
                        Some(&edit.task),
                        &current,
                        next,
                        None,
                        edit.task
                            .assigned_tab_id
                            .as_deref()
                            .or(edit.task.lane.tab_id.as_deref())
                            .is_some_and(|tab| !tab.trim().is_empty()),
                        |_| Ok(()),
                        || Ok(()),
                    )?;
                }
                let task = &mut edit.task;
                if task.lane.branch.as_deref().unwrap_or("").is_empty() {
                    task.lane.branch = report
                        .branch
                        .clone()
                        .filter(|branch| !branch.trim().is_empty());
                }
                let milestone_id = crate::plan::ids::short_id("ms");
                task.milestones.push(Milestone {
                    id: milestone_id.clone(),
                    from: from.clone(),
                    kind: "worker_report".into(),
                    text: match report.status {
                        Some(ReportStatus::Blocked) => {
                            format!("BLOCK PROPOSAL: {}", report.message)
                        }
                        Some(status) => format!("{}: {}", status.label(), report.message),
                        None => report.message.clone(),
                    },
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
                let owner_epoch = task.interruption.owner_epoch;
                let report_attempt_index =
                    task.execution_attempts.iter_mut().rposition(|attempt| {
                        attempt.executor_tab_id == reporter_tab
                            && attempt.closed_at.is_none()
                            && (owner_epoch == 0 || attempt.interruption.owner_epoch == owner_epoch)
                    });
                if let Some(index) = report_attempt_index {
                    let attempt = &mut task.execution_attempts[index];
                    attempt.report = Some(TaskExecutionReportSnapshot {
                        id: format!("report_{now_ms}"),
                        message: Some(report.message),
                        status: report.status.map(super::report_status_key),
                        sha: report.sha,
                        reporter_tab_id: None,
                        changed_files: report.changed_files,
                        test_cmd: report.test_cmd,
                        test_result: report.test_result,
                        skipped: report.skipped,
                        attachments: vec![],
                        plan_skill_manifest: attempt.plan_skill_manifest.clone(),
                        loaded_plan_skills: attempt.loaded_plan_skills.clone(),
                    });
                    attempt.reported_at = Some(now_ms);
                    attempt.report_target_written_at = None;
                    if attempt.remote_dispatch_state.is_some() {
                        attempt.remote_dispatch_state =
                            Some(crate::plan::RemoteDispatchState::Reported);
                    }
                }
                let entering_review = committed_review && task.status != TaskStatus::InReview;
                if let Some(next) = accepted_transition.as_ref() {
                    if &task.status != next {
                        task.status = next.clone();
                        task.status_changed_at = now_ms;
                    }
                }
                if entering_review {
                    crate::service_plan::qa::normalize_task_review_fields(task);
                    if let Some(index) = report_attempt_index {
                        crate::service_plan::review_gate::reset_completed_review_continuation(
                            task, index,
                        )?;
                    }
                    crate::service_plan::review_gate::fix_oracle_and_consume_override(task, now_ms);
                }
                task.updated_at = now_ms;
                if entering_review {
                    edit.review_credit -= 1;
                }
                Ok(BoundWorkerReportReceipt {
                    committed_review,
                    milestone_id: Some(milestone_id),
                })
            },
        )
    }

    pub fn mark_report_target_written(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Attempts,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let Some(attempt) = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                else {
                    return Ok(false);
                };
                if attempt.report.is_none() || attempt.report_target_written_at.is_some() {
                    return Ok(false);
                }
                attempt.report_target_written_at = Some(now_ms);
                Ok(true)
            },
        )
    }
}
