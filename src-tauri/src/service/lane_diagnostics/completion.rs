use super::*;

impl crate::service::ServiceContext {
    pub(crate) fn enqueue_lane_diagnostics(&self, request: DiagnosticsRequest) {
        let completion_owner = self.lane_diagnostics.owner();
        let completion_request = request.clone();
        let log_plan = request.plan_id.clone();
        let log_task = request.task_id.clone();
        if !self.lane_diagnostics.enqueue(request, move |result| {
            if let Some(service) = completion_owner.upgrade() {
                service.complete_lane_diagnostics(completion_request, result);
            }
        }) {
            log::warn!(
                "[lane-diagnostics] scheduler closed for {}/{}",
                log_plan,
                log_task
            );
        }
    }

    fn complete_lane_diagnostics(
        &self,
        request: DiagnosticsRequest,
        result: Result<LaneDiagnosticsSummary, String>,
    ) {
        let summary = match result {
            Ok(summary) => summary,
            Err(error) => {
                let worktree = warm_worktree_path(&request.canonical_project_path)
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| request.lane_worktree_path.clone());
                let summary = LaneDiagnosticsSummary::incomplete(
                    request.plan_id.clone(),
                    request.task_id.clone(),
                    request.attempt_id.clone(),
                    worktree,
                    format!("diagnostics incomplete: {error}"),
                );
                if let Err(storage_error) = self.lane_diagnostics.store_summary(&summary) {
                    log::warn!(
                        "[lane-diagnostics] could not store incomplete summary: {storage_error}"
                    );
                }
                summary
            }
        };
        match self.plan_store.record_task_attempt_diagnostics(
            &request.plan_id,
            &request.task_id,
            &request.attempt_id,
            summary.clone(),
            diagnostics_now_ms(),
        ) {
            Ok(true) => crate::api::events::emit_everywhere(
                &self.app_handle,
                "plan-changed",
                serde_json::json!({ "plan_id": request.plan_id.clone() }),
            ),
            Ok(false) => log::warn!(
                "[lane-diagnostics] attempt {} no longer has a report",
                request.attempt_id
            ),
            Err(error) => log::warn!(
                "[lane-diagnostics] attempt {} update failed: {error}",
                request.attempt_id
            ),
        }
    }

    pub(crate) fn diagnostics_request_for_report(
        &self,
        tab_id: &str,
        plan_id: &str,
        task_id: &str,
        details: &crate::agent::reports::ReportDetails,
    ) -> Option<DiagnosticsRequest> {
        let plan = self.plan_store.get(plan_id)?;
        let task = plan.tasks.iter().find(|task| task.id == task_id)?;
        let lane = task
            .lane
            .worktree_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())?;
        let marker = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.provisioned_cwd.clone())?;
        if !crate::service::same_path(lane, &marker) {
            return None;
        }
        let attempt_id = details.attempt_id.clone().or_else(|| {
            task.execution_attempts
                .iter()
                .rev()
                .find(|attempt| attempt.executor_tab_id == tab_id && attempt.closed_at.is_none())
                .map(|attempt| attempt.id.clone())
        })?;
        Some(DiagnosticsRequest {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id,
            canonical_project_path: canonical_project_path(&plan),
            lane_worktree_path: lane.to_string(),
            lane_marker: Some(marker),
            lane_branch: details.branch.clone().or_else(|| task.lane.branch.clone()),
            base: details.base.clone().or_else(|| task.lane.base.clone()),
            sha: details.sha.clone(),
            changed_files: details.changed_files.clone().unwrap_or_default(),
            require_lane_marker: true,
        })
    }

    pub(crate) fn check_lane_subject(
        &self,
        plan_ref: Option<&str>,
        subject: &str,
    ) -> Result<(String, String, LaneDiagnosticsSummary), String> {
        let (plan, task) = resolve_lane_subject(&self.plan_store, plan_ref, subject)?;
        let attempt_id = task
            .execution_attempts
            .iter()
            .rev()
            .find(|attempt| attempt.closed_at.is_none())
            .map(|attempt| attempt.id.clone())
            .unwrap_or_else(|| "manual".into());
        let lane = task
            .lane
            .worktree_path
            .clone()
            .ok_or_else(|| "task has no host-owned lane worktree".to_string())?;
        let request = DiagnosticsRequest {
            plan_id: plan.id.clone(),
            task_id: task.id.clone(),
            attempt_id,
            canonical_project_path: canonical_project_path(&plan),
            lane_worktree_path: lane,
            lane_marker: None,
            lane_branch: task.lane.branch.clone(),
            base: task.lane.base.clone(),
            sha: None,
            changed_files: Vec::new(),
            require_lane_marker: false,
        };
        let summary = self.lane_diagnostics.run(request)?;
        Ok((plan.id, task.id, summary))
    }

    pub(crate) fn diagnostics_for_task(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<LaneDiagnosticsSummary, String> {
        let plan = self
            .plan_store
            .get(plan_id)
            .ok_or_else(|| "plan not found".to_string())?;
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| "task not found".to_string())?;
        if let Some(summary) = task
            .execution_attempts
            .iter()
            .rev()
            .filter_map(|attempt| attempt.report.as_ref())
            .find_map(|report| report.diagnostics.clone())
        {
            return Ok(summary);
        }
        let attempt_id = task
            .execution_attempts
            .iter()
            .rev()
            .map(|attempt| attempt.id.as_str())
            .next()
            .ok_or_else(|| "task has no execution attempt".to_string())?;
        self.lane_diagnostics.load(plan_id, task_id, attempt_id)
    }
}
