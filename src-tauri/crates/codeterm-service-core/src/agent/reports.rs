use super::ledger::OrchestrationLedger;
pub use codeterm_agent_model::agent::reports::*;
use std::sync::Arc;
type ModelReportStore = codeterm_agent_model::agent::reports::ReportStore;

pub struct ReportStore(ModelReportStore);

impl std::ops::Deref for ReportStore {
    type Target = ModelReportStore;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ReportStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl ReportStore {
    pub fn new(max_per_orchestrator: usize) -> Self {
        Self(ModelReportStore::new(max_per_orchestrator))
    }
    pub fn with_ledger(max_per_orchestrator: usize, ledger: Arc<OrchestrationLedger>) -> Self {
        Self(ModelReportStore::with_ledger(max_per_orchestrator, ledger))
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
mod tests {
    use super::*;

    #[test]
    fn report_delivery_traces_only_the_first_written_transition() {
        let mut store = ReportStore::new(4);
        let report = store.add("target", "sender", "sender-title", "opaque-body");
        let events = crate::flow_trace::test_capture::capture(|| {
            store.mark_delivered(&report.id);
            store.mark_delivered(&report.id);
            store.mark_delivered("unknown");
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage, "report.written");
        assert_eq!(events[0].tab, "target");
    }
}
