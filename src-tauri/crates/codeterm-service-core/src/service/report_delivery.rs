use crate::agent::reports::AgentReport;
use crate::service::startup_nudge::StartupNudge;
use crate::service::ServiceContext;

pub trait HierarchyReportDelivery {
    fn deliver(&self, recipient_tab_id: &str, report: &AgentReport) -> Result<bool, String>;
}

impl HierarchyReportDelivery for ServiceContext {
    fn deliver(&self, recipient_tab_id: &str, report: &AgentReport) -> Result<bool, String> {
        let mut envelope = crate::service::messenger::MessageEnvelope::report(
            &report.from_title,
            &report.from_tab_id,
        )
        .model(report.model.clone());
        envelope.id = Some(report.id.clone());
        envelope.delivery_observer =
            Some(crate::service::messenger::DeliveryWriteObserver::Report {
                report_id: report.id.clone(),
                plan_id: report.plan_id.clone(),
                task_id: report.task_id.clone(),
                attempt_id: report.attempt_id.clone(),
            });
        self.gateway_deliver_with_shares(
            recipient_tab_id,
            envelope,
            &report.inject_body(),
            crate::service::message_gateway::DeliveryClass::AgentTraffic,
            Some(&super::local_preview::PadAttachments::default()),
        )
        .map(|(delivery, _)| delivery.is_written())
    }
}

pub struct HierarchyReportRecipientGate {
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
    pub fn hierarchy_report_recipient_gate(
        &self,
        recipient_tab_id: &str,
    ) -> HierarchyReportRecipientGate {
        HierarchyReportRecipientGate {
            recipient_tab_id: recipient_tab_id.to_string(),
            nudge: self.classify_startup_nudge_for_hierarchy_report(recipient_tab_id),
        }
    }

    pub fn deliver_hierarchy_report_if_allowed(
        &self,
        gate: &HierarchyReportRecipientGate,
        report: &AgentReport,
        delivery: &impl HierarchyReportDelivery,
    ) -> bool {
        if !gate.allows_report(&report.id) || !self.report_store.lock().claim(&report.id) {
            return false;
        }
        match delivery.deliver(&gate.recipient_tab_id, report) {
            Ok(true) => {
                self.report_store.lock().mark_delivered(&report.id);
                log::info!(
                    "[report/lifecycle] target={} report={} outcome=written",
                    gate.recipient_tab_id,
                    report.id
                );
                true
            }
            Ok(false) => {
                log::warn!(
                    "[report/lifecycle] target={} report={} outcome=pending",
                    gate.recipient_tab_id,
                    report.id
                );
                false
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

impl ServiceContext {
    pub fn replay_pending_reports_with_delivery(
        &self,
        tab_id: &str,
        delivery: &impl HierarchyReportDelivery,
    ) {
        let pending = self.report_store.lock().pending_for(tab_id);
        if pending.is_empty() {
            return;
        }
        let gate = self.hierarchy_report_recipient_gate(tab_id);
        for report in pending {
            self.deliver_hierarchy_report_if_allowed(&gate, &report, delivery);
        }
    }
}
