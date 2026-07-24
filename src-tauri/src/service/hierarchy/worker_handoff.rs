use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::service::ServiceContext;

impl ServiceContext {
    pub(crate) fn complete_worker_turn_without_report(&self, tab_id: &str, source: &str) {
        if !self.gateway.take_worker_report_requirement(tab_id) {
            return;
        }
        let message = format!(
            "STATUS: partial\n\
             CHANGES: Worker turn ended without an explicit codeterm agent report\n\
             TESTS RUN: not reported\n\
             GATES REQUESTED: none\n\
             FOLLOWUP: automatic {source} handoff; task remains in progress"
        );
        let details = ReportDetails {
            status: Some(ReportStatus::Partial),
            ..Default::default()
        };
        if let Err(error) = self.submit_report(tab_id, &message, details, None, None, None) {
            self.gateway.require_worker_report(tab_id);
            log::warn!(
                "[worker-report-fallback] tab={} source={} failed: {}",
                tab_id,
                source,
                error
            );
        }
    }
}
