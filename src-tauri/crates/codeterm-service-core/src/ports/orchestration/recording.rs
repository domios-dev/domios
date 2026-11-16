use super::*;
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct RecordingOrchestration {
    pub calls: parking_lot::Mutex<Vec<&'static str>>,
    pub closed_teams: parking_lot::Mutex<Vec<(String, TeamMutationCaller, bool)>>,
}

#[cfg(any(test, feature = "test-support"))]
impl OrchestrationPort for RecordingOrchestration {
    fn curator_identity(
        &self,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<CuratorIdentity, String> {
        let _ = caller;
        self.calls.lock().push("curator_identity");
        Err("recording orchestration port".into())
    }
    fn schedule_planner_replay(&self, tab_id: &str) {
        let _ = (tab_id,);
        self.calls.lock().push("schedule_planner_replay");
        ()
    }
    fn bind_planner_session(&self, tab_id: &str) -> Result<(), String> {
        let _ = (tab_id,);
        self.calls.lock().push("bind_planner_session");
        Ok(())
    }
    fn submit_report(
        &self,
        tab_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, ReportSubmissionError> {
        let _ = (tab_id, message, details, plan_id, task_id, milestone_kind);
        self.calls.lock().push("submit_report");
        Err("recording orchestration port".to_string().into())
    }
    fn reconcile_time_reminder_eligibility(&self) {
        self.calls
            .lock()
            .push("reconcile_time_reminder_eligibility");
        ()
    }
    fn expect_spawn_parent_link(&self, tab_id: &str, expected_parent: &str) -> Result<(), String> {
        let _ = (tab_id, expected_parent);
        self.calls.lock().push("expect_spawn_parent_link");
        Ok(())
    }
    fn clear_dead_planner_binding(&self, tab_id: &str) {
        let _ = (tab_id,);
        self.calls.lock().push("clear_dead_planner_binding");
        ()
    }
    fn curator_suspended_reason_for_fields(
        &self,
        mode: &PlanMode,
        status: &PlanStatus,
        team_id: &Option<String>,
        complete: bool,
    ) -> Option<CuratorSuspension> {
        let _ = (mode, status, team_id, complete);
        self.calls
            .lock()
            .push("curator_suspended_reason_for_fields");
        None
    }
    fn heartbeat_body_for_popup(
        &self,
        plan: &ReminderCompositionRead,
        tab_id: &str,
        worker: bool,
    ) -> Result<String, String> {
        let _ = (plan, tab_id, worker);
        self.calls.lock().push("heartbeat_body_for_popup");
        Ok(String::new())
    }
    fn curator_turn_settled(&self, tab_id: &str) {
        let _ = (tab_id,);
        self.calls.lock().push("curator_turn_settled");
        ()
    }
    fn reconcile_curator_id(
        &self,
        id: &str,
        now: u64,
    ) -> Result<(), crate::service::plan::PlanError> {
        let _ = (id, now);
        self.calls.lock().push("reconcile_curator_id");
        Ok(())
    }
    fn complete_worker_turn_without_report(&self, tab_id: &str, signal: WorkerTurnSignal) {
        let _ = (tab_id, signal);
        self.calls
            .lock()
            .push("complete_worker_turn_without_report");
        ()
    }
    fn send_refused_report_diagnostic(
        &self,
        reporter_tab_id: &str,
        message: &str,
    ) -> Result<String, String> {
        let _ = (reporter_tab_id, message);
        self.calls.lock().push("send_refused_report_diagnostic");
        Ok(String::new())
    }
    fn start_plan_reminder_scheduler(&self) {
        self.calls.lock().push("start_plan_reminder_scheduler");
        ()
    }
    fn deliver_tab_todo_recitation(&self, tab_id: &str, text: &str) -> Result<bool, String> {
        let _ = (tab_id, text);
        self.calls.lock().push("deliver_tab_todo_recitation");
        Ok(false)
    }
    fn deliver_orchestrator_notice_exact(
        &self,
        plan_id: &str,
        requested_target: &str,
        context: &str,
        text: &str,
    ) -> Result<Delivery, String> {
        let _ = (plan_id, requested_target, context, text);
        self.calls.lock().push("deliver_orchestrator_notice_exact");
        Err("recording orchestration port".into())
    }
    fn curator_policy_authorize_notification(&self, tab: &str) -> Result<(), String> {
        let _ = (tab,);
        self.calls
            .lock()
            .push("curator_policy_authorize_notification");
        Err("recording orchestration port".into())
    }
    fn curator_policy_session_claimed(&self, tab: &str) -> Result<(), String> {
        let _ = (tab,);
        self.calls.lock().push("curator_policy_session_claimed");
        Ok(())
    }
    fn curator_policy_tab_closed(&self, tab: &str) -> Result<(), String> {
        let _ = (tab,);
        self.calls.lock().push("curator_policy_tab_closed");
        Ok(())
    }
    fn curator_policy_settle_cron(
        &self,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<CronSettlement, String> {
        let _ = (job, tab, run, now);
        self.calls.lock().push("curator_policy_settle_cron");
        Err("recording orchestration port".into())
    }
    fn curator_policy_prepare_cron(
        &self,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<CronLaunch>, String> {
        let _ = (job, now);
        self.calls.lock().push("curator_policy_prepare_cron");
        Ok(None)
    }
    fn curator_policy_launch_failed(&self, job: &CronJob, now: u64) -> Result<(), String> {
        let _ = (job, now);
        self.calls.lock().push("curator_policy_launch_failed");
        Ok(())
    }
    fn curator_policy_spawned(
        &self,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<(), String> {
        let _ = (job, tab, run, now);
        self.calls.lock().push("curator_policy_spawned");
        Ok(())
    }
    fn curator_policy_prepare_message(
        &self,
        sender: &str,
        target: &str,
        body: &str,
    ) -> Result<Option<String>, String> {
        let _ = (sender, target, body);
        self.calls.lock().push("curator_policy_prepare_message");
        Ok(None)
    }
    fn curator_policy_message_receipt(
        &self,
        sender: &str,
        receipt: &Result<DeliveryReceipt, DeliveryError>,
    ) -> Result<(), String> {
        let _ = (sender, receipt);
        self.calls.lock().push("curator_policy_message_receipt");
        Ok(())
    }
    fn curator_policy_authorize_blueprint(
        &self,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String> {
        let _ = (tab, id, action);
        self.calls.lock().push("curator_policy_authorize_blueprint");
        Ok(false)
    }
    fn curator_policy_learning_owner(
        &self,
        caller: &AuthenticatedPlanCaller,
        plan_id: &str,
        tag: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String> {
        let _ = (caller, plan_id, tag);
        self.calls.lock().push("curator_policy_learning_owner");
        Ok((None, None))
    }
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture {
        self.closed_teams.lock().push((team_id, caller, force));
        Box::pin(async { Ok(()) })
    }
}
