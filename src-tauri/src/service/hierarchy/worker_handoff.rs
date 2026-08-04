use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::service::message_gateway::MessageGateway;
use crate::service::ServiceContext;

#[derive(Clone, Copy)]
pub(crate) enum WorkerTurnSignal {
    ProviderStop,
    IdleTransition,
}

impl WorkerTurnSignal {
    fn source(self) -> &'static str {
        match self {
            Self::ProviderStop => "provider-stop",
            Self::IdleTransition => "idle-transition",
        }
    }
}

fn take_report_requirement_for_signal(
    gateway: &MessageGateway,
    tab_id: &str,
    signal: WorkerTurnSignal,
) -> bool {
    match signal {
        WorkerTurnSignal::ProviderStop => gateway.take_worker_report_requirement(tab_id),
        WorkerTurnSignal::IdleTransition => {
            log::debug!(
                "[worker-report-fallback] tab={} idle without an explicit completion",
                tab_id
            );
            false
        }
    }
}

pub(super) struct ExplicitReportClaim<'a> {
    gateway: &'a MessageGateway,
    tab_id: &'a str,
    claimed: bool,
    accepted: bool,
}

impl ExplicitReportClaim<'_> {
    pub(super) fn accept(&mut self) {
        self.accepted = true;
    }
}

impl Drop for ExplicitReportClaim<'_> {
    fn drop(&mut self) {
        if self.claimed && !self.accepted {
            self.gateway.require_worker_report(self.tab_id);
        }
    }
}

pub(super) fn claim_explicit_report<'a>(
    gateway: &'a MessageGateway,
    tab_id: &'a str,
) -> ExplicitReportClaim<'a> {
    ExplicitReportClaim {
        gateway,
        tab_id,
        claimed: gateway.take_worker_report_requirement(tab_id),
        accepted: false,
    }
}

fn fallback_report_message(source: &str) -> String {
    format!(
        "STATUS: partial\n\
         CHANGES: Worker turn ended without an explicit codeterm agent report\n\
         TESTS RUN: not reported\n\
         GATES REQUESTED: none\n\
         FOLLOWUP: automatic {source} handoff; task remains in progress"
    )
}

/// Absent or mismatched binding evidence stays absent, so persistence fails
/// closed instead of inventing an authority the Worker never proved.
fn observed_handoff_authority(svc: &ServiceContext, tab_id: &str) -> ReportDetails {
    let session_binding_nonce = svc.mux.read().get_tab(tab_id).and_then(|tab| {
        let reported = tab.reported_session_binding_nonce.clone()?;
        (tab.session_binding_nonce.as_deref() == Some(reported.as_str())).then_some(reported)
    });
    let owner_epoch = svc
        .plan_store
        .task_binding_for_tab(tab_id)
        .and_then(|(_, task_id)| svc.plan_store.show_task(&task_id))
        .map(|detail| detail.task.interruption.owner_epoch);
    ReportDetails {
        status: Some(ReportStatus::Partial),
        caller_tab_id: Some(tab_id.to_string()),
        session_binding_nonce,
        owner_epoch,
        ..Default::default()
    }
}

impl ServiceContext {
    pub(crate) fn complete_worker_turn_without_report(
        &self,
        tab_id: &str,
        signal: WorkerTurnSignal,
    ) {
        if !take_report_requirement_for_signal(&self.gateway, tab_id, signal) {
            return;
        }
        let source = signal.source();
        let message = fallback_report_message(source);
        let details = observed_handoff_authority(self, tab_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_transition_preserves_report_requirement_in_both_report_orderings() {
        let gateway = MessageGateway::new();

        gateway.require_worker_report("idle-then-report");
        assert!(!take_report_requirement_for_signal(
            &gateway,
            "idle-then-report",
            WorkerTurnSignal::IdleTransition,
        ));
        let mut claim = claim_explicit_report(&gateway, "idle-then-report");
        assert!(claim.claimed);
        claim.accept();
        drop(claim);
        assert!(!gateway.take_worker_report_requirement("idle-then-report"));

        gateway.require_worker_report("report-then-idle");
        let mut claim = claim_explicit_report(&gateway, "report-then-idle");
        assert!(claim.claimed);
        claim.accept();
        assert!(!take_report_requirement_for_signal(
            &gateway,
            "report-then-idle",
            WorkerTurnSignal::IdleTransition,
        ));
        drop(claim);
        assert!(!gateway.take_worker_report_requirement("report-then-idle"));
    }

    #[test]
    fn provider_stop_consumes_report_requirement_for_synthetic_handoff() {
        let gateway = MessageGateway::new();
        gateway.require_worker_report("finished-worker");

        assert!(take_report_requirement_for_signal(
            &gateway,
            "finished-worker",
            WorkerTurnSignal::ProviderStop,
        ));
        assert!(!gateway.take_worker_report_requirement("finished-worker"));
        assert_eq!(
            fallback_report_message(WorkerTurnSignal::ProviderStop.source()),
            "STATUS: partial\n\
             CHANGES: Worker turn ended without an explicit codeterm agent report\n\
             TESTS RUN: not reported\n\
             GATES REQUESTED: none\n\
             FOLLOWUP: automatic provider-stop handoff; task remains in progress"
        );
    }

    #[test]
    fn rejected_explicit_report_restores_report_requirement() {
        let gateway = MessageGateway::new();
        gateway.require_worker_report("rejected-report");

        drop(claim_explicit_report(&gateway, "rejected-report"));

        assert!(gateway.take_worker_report_requirement("rejected-report"));
    }
}
