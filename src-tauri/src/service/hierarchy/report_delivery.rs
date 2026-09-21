use crate::agent::reports::AgentReport;
use crate::service::startup_nudge::StartupNudge;
use crate::service::ServiceContext;

pub(crate) trait HierarchyReportDelivery {
    fn deliver(&self, recipient_tab_id: &str, report: &AgentReport) -> Result<(), String>;
}

impl HierarchyReportDelivery for ServiceContext {
    fn deliver(&self, recipient_tab_id: &str, report: &AgentReport) -> Result<(), String> {
        let envelope = crate::service::messenger::MessageEnvelope::report(
            &report.from_title,
            &report.from_tab_id,
        )
        .model(report.model.clone());
        self.gateway_deliver(
            recipient_tab_id,
            envelope,
            &report.inject_body(),
            crate::service::message_gateway::DeliveryClass::AgentTraffic,
        )
        .map(|_| ())
    }
}

pub(crate) struct HierarchyReportRecipientGate {
    recipient_tab_id: String,
    nudge: StartupNudge,
}

impl HierarchyReportRecipientGate {
    fn allows_report(&self, report_id: &str) -> bool {
        if self.nudge.delivers() {
            return true;
        }
        log::warn!(
            "[report/lifecycle] target={} report={} withheld: {}",
            self.recipient_tab_id,
            report_id,
            self.nudge.reason()
        );
        false
    }
}

impl ServiceContext {
    pub(crate) fn hierarchy_report_recipient_gate(
        &self,
        recipient_tab_id: &str,
    ) -> HierarchyReportRecipientGate {
        HierarchyReportRecipientGate {
            recipient_tab_id: recipient_tab_id.to_string(),
            nudge: self.classify_startup_nudge_for_hierarchy_report(recipient_tab_id),
        }
    }

    pub(crate) fn deliver_hierarchy_report_if_allowed(
        &self,
        gate: &HierarchyReportRecipientGate,
        report: &AgentReport,
        delivery: &impl HierarchyReportDelivery,
    ) -> bool {
        if !gate.allows_report(&report.id) || !self.report_store.lock().claim(&report.id) {
            return false;
        }
        match delivery.deliver(&gate.recipient_tab_id, report) {
            Ok(()) => {
                self.report_store.lock().mark_delivered(&report.id);
                log::info!(
                    "[report/lifecycle] target={} report={} delivered",
                    gate.recipient_tab_id,
                    report.id
                );
                true
            }
            Err(error) => {
                self.report_store.lock().release_delivery(&report.id);
                if let Some(task_id) = &report.task_id {
                    log::warn!(
                        "[report/lifecycle] target={} report={} task={} tab={} deferred: {}",
                        gate.recipient_tab_id,
                        report.id,
                        task_id,
                        report.from_tab_id,
                        error
                    );
                } else {
                    log::warn!(
                        "[report/lifecycle] target={} report={} deferred: {}",
                        gate.recipient_tab_id,
                        report.id,
                        error
                    );
                }
                false
            }
        }
    }
}
