use super::*;

pub trait HierarchyExt {
    fn schedule_planner_replay(&self, tab_id: &str);
    fn replay_pending_reports_if_routable(&self, tab_id: &str);
    fn clear_dead_planner_binding(&self, tab_id: &str);
    fn replay_planner_requests_if_routable(&self, tab_id: &str);
    fn deliver_to_routable_planner(
        &self,
        plan_id: &str,
        tab_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        context: &str,
        body: &str,
        message_id: String,
    ) -> Result<(), String>;
    fn acknowledge_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        tab_id: &str,
    ) -> Result<codeterm_service_core::plan::PlannerRequest, String>;
    fn set_can_spawn(&self, tab_id: &str, can_spawn: bool) -> Result<(), String>;
    fn expect_spawn_parent_link(&self, tab_id: &str, expected_parent: &str) -> Result<(), String>;
    fn submit_report(
        &self,
        tab_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, ReportSubmissionError>;
    fn submit_report_with_source_outcome(
        &self,
        tab_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
        source_outcome: Option<ReportOutcomeClass>,
    ) -> Result<ReportResult, ReportSubmissionError>;
    fn deliver_remote_task_report(&self, delivery: RemoteTaskReportDelivery<'_>) -> String;
    fn notify_owner_task_in_review(
        &self,
        plan_id: &str,
        task_id: &str,
        by_tab: Option<&str>,
        intent: Option<&str>,
    );
    fn nudge_gate_asker(
        &self,
        plan_id: &str,
        gate_id: &str,
        answer_text: &str,
        answered_by_tab: Option<&str>,
    );
    fn relay_plan_batch_to_peer(
        &self,
        peer: String,
        plan_id: String,
        task_id: String,
        from_ref: AgentRef,
        task_status: Option<&'static str>,
        status_label: Option<String>,
        ms_text: String,
        ms_kind: String,
    ) -> Result<(), String>;
    fn get_reports(&self, tab_id: &str) -> Vec<serde_json::Value>;
    fn get_and_clear_reports(&self, tab_id: &str) -> Vec<serde_json::Value>;
    fn get_ledger(&self, orchestrator_tab_id: &str) -> Vec<LedgerRow>;
}
