use super::{BoundWorkerReport, PlanError, PlanStore};
use crate::plan::{AgentRef, Milestone, TaskExecutionReportSnapshot, TaskStatus};
use codeterm_types::agent::ReportStatus;

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
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
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
                if attempt.remote_dispatch_state.is_some() {
                    attempt.remote_dispatch_state = Some(crate::plan::RemoteDispatchState::Reported);
                }
            }
            Ok(())
        })
    }

    /// Ingest a worker report for its currently assigned task in one plan write.
    /// Accepted report status and task state are changed in the same plan write.
    pub fn ingest_bound_worker_report(
        &self,
        plan_id: &str,
        task_id: &str,
        from: &AgentRef,
        report: BoundWorkerReport,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let Some(reporter_tab) = from.tab_id.as_deref() else {
            return Ok(false);
        };
        let transition = report
            .status
            .map(ReportStatus::task_transition)
            .transpose()
            .map_err(|error| PlanError::Io(error.into()))?
            .flatten();
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if plan.tasks[task_index].assigned_tab_id.as_deref() != Some(reporter_tab) {
                return Ok(false);
            }
            crate::service_plan::task_attempt_gates::validate_authorized_gate_claim(
                &plan.tasks[task_index],
                reporter_tab,
                report.test_cmd.as_deref(),
            )?;
            let interruption = &plan.tasks[task_index].interruption;
            if interruption.handoff_required
                || (interruption.owner_epoch > 0
                    && interruption.owner_tab_id.as_deref() != Some(reporter_tab))
            {
                return Ok(false);
            }
            if interruption.owner_epoch > 0 {
                let authority = report
                    .authority
                    .as_ref()
                    .ok_or_else(|| PlanError::Io("report caller authority is missing".into()))?;
                crate::service_plan::interruption::authorize_task_mutation(
                    plan,
                    &plan.tasks[task_index],
                    authority,
                )?;
                if interruption.owner_tab_id.as_deref() == Some(&authority.tab_id) {
                    let exact_attempt =
                        plan.tasks[task_index]
                            .execution_attempts
                            .iter()
                            .any(|attempt| {
                                attempt.executor_tab_id == authority.tab_id
                                    && attempt.interruption.owner_epoch == authority.owner_epoch
                                    && attempt.closed_at.is_none()
                            });
                    if !exact_attempt {
                        return Err(PlanError::Io("report attempt authority changed".into()));
                    }
                }
            }
            if plan.tasks[task_index]
                .execution_attempts
                .iter()
                .filter(|attempt| attempt.executor_tab_id == reporter_tab)
                .filter_map(|attempt| attempt.reported_at)
                .any(|reported_at| reported_at >= now_ms)
            {
                return Ok(false);
            }
            let current = plan.tasks[task_index].status.clone();
            if matches!(current, TaskStatus::Done | TaskStatus::Cancelled) {
                return Ok(false);
            }
            let accepted_transition =
                if current == TaskStatus::InReview && transition == Some(TaskStatus::Blocked) {
                    None
                } else {
                    transition.clone()
                };
            let committed_review = accepted_transition == Some(TaskStatus::InReview);
            let blocked_reason =
                (accepted_transition == Some(TaskStatus::Blocked)).then(|| report.message.clone());
            if let Some(next) = accepted_transition
                .as_ref()
                .filter(|next| *next != &current)
            {
                let incoming_label =
                    (next == &TaskStatus::Blocked).then_some(Some(report.message.as_str()));
                crate::service_plan::mutations::validate_task_status_transition(
                    plan,
                    &plan.tasks[task_index].id,
                    &current,
                    next,
                    incoming_label,
                )?;
            }
            let task = &mut plan.tasks[task_index];
            if task.lane.branch.as_deref().unwrap_or("").is_empty() {
                task.lane.branch = report
                    .branch
                    .clone()
                    .filter(|branch| !branch.trim().is_empty());
            }
            task.milestones.push(Milestone {
                id: crate::plan::ids::short_id("ms"),
                from: from.clone(),
                kind: "worker_report".into(),
                text: match report.status {
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
            let report_attempt_index = task.execution_attempts.iter_mut().rposition(|attempt| {
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
                if attempt.remote_dispatch_state.is_some() {
                    attempt.remote_dispatch_state = Some(crate::plan::RemoteDispatchState::Reported);
                }
            }
            let entering_review = committed_review && task.status != TaskStatus::InReview;
            if let Some(next) = accepted_transition.as_ref() {
                if &task.status != next {
                    task.status = next.clone();
                    task.status_changed_at = now_ms;
                }
                if next == &TaskStatus::Blocked {
                    task.status_label = blocked_reason.clone();
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
                crate::service_plan::review_gate::spend_review_credit(&mut plan.review_policy);
            }
            Ok(committed_review)
        })
    }
}
