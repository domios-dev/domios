use super::ledger::OrchestrationLedger;
pub use codeterm_agent_model::agent::reports::{
    AgentReport, ReportDetails, ReportModelIdentity, ReportStatus, BLOCK_PROPOSAL_NOTICE,
};
use std::sync::Arc;
type ModelReportStore = codeterm_agent_model::agent::reports::ReportStore;

pub struct ReportStore(ModelReportStore);

impl std::ops::Deref for ReportStore {
    type Target = ModelReportStore;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl ReportStore {
    pub fn new(max_per_orchestrator: usize) -> Self {
        Self(ModelReportStore::new(max_per_orchestrator))
    }
    pub fn with_ledger(max_per_orchestrator: usize, ledger: Arc<OrchestrationLedger>) -> Self {
        Self(ModelReportStore::with_ledger(max_per_orchestrator, ledger))
    }
    pub fn add(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
    ) -> AgentReport {
        self.0
            .add(orchestrator_tab_id, from_tab_id, from_title, message)
    }
    pub fn add_detailed(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model: Option<&str>,
    ) -> AgentReport {
        self.0.add_detailed(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model,
        )
    }
    pub fn add_detailed_with_model_identity(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model_identity: Option<ReportModelIdentity>,
    ) -> AgentReport {
        self.0.add_detailed_with_model_identity(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model_identity,
        )
    }
    pub fn add_detailed_with_model_identity_and_outcome(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model_identity: Option<ReportModelIdentity>,
        report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
    ) -> AgentReport {
        self.0.add_detailed_with_model_identity_and_outcome(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model_identity,
            report_outcome,
        )
    }
    pub fn get(&mut self, orchestrator_tab_id: &str, clear: bool) -> Vec<AgentReport> {
        self.0.get(orchestrator_tab_id, clear)
    }
    pub fn claim_pending_for(&mut self, orchestrator_tab_id: &str) -> Vec<AgentReport> {
        self.0.claim_pending_for(orchestrator_tab_id)
    }
    pub fn claim(&mut self, report_id: &str) -> bool {
        self.0.claim(report_id)
    }
    pub fn release_delivery(&mut self, report_id: &str) {
        self.0.release_delivery(report_id);
    }
    pub fn remove(&mut self, orchestrator_tab_id: &str) {
        self.0.remove(orchestrator_tab_id);
    }
    pub fn mark_delivered(&mut self, report_id: &str) {
        if let Some((target, attempt)) = self.0.delivery_trace(report_id) {
            crate::flow_trace::record_flow(
                report_id,
                "report.written",
                target,
                "",
                "delivered",
                attempt,
            );
        }
        self.0.mark_delivered(report_id);
    }
}
#[cfg(test)]
mod tests;
