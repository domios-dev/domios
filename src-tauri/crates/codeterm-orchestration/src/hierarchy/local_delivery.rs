use codeterm_service_core::agent::reports::{
    AgentReport, AgentReportHostMetadata, ReportDetails, ReportModelIdentity, ReportStatus,
};
use codeterm_service_core::plan::TaskExecutionReportSnapshot;
use codeterm_service_core::service::{RejectedTaskTransition, ServiceContext};
use codeterm_types::agent::ReportOutcomeClass;

use super::report_event;

pub struct RemoteTaskReportDelivery<'a> {
    pub target_tab_id: &'a str,
    pub from_tab_id: &'a str,
    pub from_title: &'a str,
    pub plan_id: &'a str,
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub model_identity: Option<ReportModelIdentity>,
    pub model: Option<String>,
    pub snapshot: &'a TaskExecutionReportSnapshot,
}

pub(super) struct RecordedReportInput<'a> {
    pub target_tab_id: &'a str,
    pub from_tab_id: &'a str,
    pub from_title: &'a str,
    pub message: &'a str,
    pub details: ReportDetails,
    pub model_identity: Option<ReportModelIdentity>,
    pub report_outcome: Option<ReportOutcomeClass>,
    pub host_metadata: AgentReportHostMetadata,
    pub persisted_task: Option<&'a (String, String)>,
    pub rejected_task_transition: Option<&'a RejectedTaskTransition>,
}

pub(super) fn record_and_emit_report(
    svc: &ServiceContext,
    input: RecordedReportInput<'_>,
) -> AgentReport {
    let report = svc
        .report_store
        .lock()
        .add_detailed_with_model_identity_and_outcome_and_host_metadata(
            input.target_tab_id,
            input.from_tab_id,
            input.from_title,
            input.message,
            input.details,
            input.model_identity,
            input.report_outcome,
            input.host_metadata,
        );
    svc.report_store.lock().claim(&report.id);
    let trace_flow = report.attempt_id.clone().or_else(|| {
        codeterm_service_core::flow_trace::diagnostic_flow(
            codeterm_service_core::flow_trace::tab_flow(svc, input.from_tab_id),
        )
    });
    codeterm_service_core::flow_trace::record_flow(
        &report.id,
        "report.emitted",
        input.from_tab_id,
        "",
        "recorded",
        trace_flow.as_deref(),
    );

    let task_report = report_event::task_report_outcome(
        input.persisted_task,
        input.rejected_task_transition,
        input.report_outcome,
    );
    let mut bus_payload = serde_json::json!({
        "target_tab_id": input.target_tab_id,
        "sender_position": report.sender_position.clone(),
        "report": report.clone(),
        "task_report": task_report.clone(),
        "report_outcome": input.report_outcome,
    });
    if let Some(kind) = report
        .kind
        .as_deref()
        .filter(|kind| !kind.trim().is_empty())
    {
        bus_payload["kind"] = serde_json::json!(kind);
    }
    svc.events.broadcast("agent.report", bus_payload);
    let mut event = report_event::agent_report_event(
        input.target_tab_id,
        input.from_tab_id,
        input.from_title,
        &report,
        task_report,
    );
    event["sender_position"] = serde_json::json!(report.sender_position);
    if let Some(kind) = report
        .kind
        .as_deref()
        .filter(|kind| !kind.trim().is_empty())
    {
        event["kind"] = serde_json::json!(kind);
    }
    svc.ui.emit("agent-report", event);
    report
}

pub(super) fn deliver_local_report_via_gateway(
    svc: &ServiceContext,
    target_id: &str,
    from_title: &str,
    from_tab_id: &str,
    model: Option<String>,
    report: &AgentReport,
) {
    let mut envelope =
        codeterm_service_core::service::messenger::MessageEnvelope::report(from_title, from_tab_id)
            .model(model);
    envelope.id = Some(report.id.clone());
    envelope.delivery_observer = Some(
        codeterm_service_core::service::messenger::DeliveryWriteObserver::Report {
            report_id: report.id.clone(),
            plan_id: report.plan_id.clone(),
            task_id: report.task_id.clone(),
            attempt_id: report.attempt_id.clone(),
        },
    );
    // Echo scraping can't decide delivery on a screen-owning TUI (it
    // reflows/collapses input) — the store+broadcast above is the guarantee.
    match svc.gateway_deliver(
        target_id,
        envelope,
        &report.inject_body(),
        codeterm_service_core::service::message_gateway::DeliveryClass::AgentTraffic,
    ) {
        Ok(
            codeterm_service_core::service::messenger::Delivery::Pending { delivery_id }
            | codeterm_service_core::service::messenger::Delivery::Unconfirmed { delivery_id },
        ) => {
            log::warn!(
                "[report] target={} outcome=pending delivery={}",
                target_id,
                delivery_id
            );
        }
        Ok(delivery) => {
            svc.report_store.lock().mark_delivered(&report.id);
            log::info!(
                "[report] target={} outcome=written via {:?}",
                target_id,
                delivery
            );
        }
        Err(error) => {
            svc.report_store.lock().release_delivery(&report.id);
            log::warn!("[report] target={} deliver: {}", target_id, error);
        }
    }
}

pub(super) fn deliver_remote_task_report(
    svc: &ServiceContext,
    delivery: RemoteTaskReportDelivery<'_>,
) -> String {
    let status = delivery
        .snapshot
        .status
        .as_deref()
        .and_then(|value| {
            serde_json::from_value::<ReportStatus>(serde_json::Value::String(value.to_string()))
                .ok()
        })
        .or(Some(ReportStatus::InReview));
    let message = delivery.snapshot.message.as_deref().unwrap_or("");
    let details = ReportDetails {
        plan_id: Some(delivery.plan_id.to_string()),
        task_id: Some(delivery.task_id.to_string()),
        attempt_id: Some(delivery.attempt_id.to_string()),
        status,
        branch: delivery.branch,
        base: delivery.base,
        sha: delivery.snapshot.sha.clone(),
        changed_files: (!delivery.snapshot.changed_files.is_empty())
            .then(|| delivery.snapshot.changed_files.clone()),
        test_cmd: delivery.snapshot.test_cmd.clone(),
        test_result: delivery.snapshot.test_result.clone(),
        skipped: delivery.snapshot.skipped.clone(),
        ..Default::default()
    };
    let persisted_task = (delivery.plan_id.to_string(), delivery.task_id.to_string());
    let report = record_and_emit_report(
        svc,
        RecordedReportInput {
            target_tab_id: delivery.target_tab_id,
            from_tab_id: delivery.from_tab_id,
            from_title: delivery.from_title,
            message,
            details,
            model_identity: delivery.model_identity,
            report_outcome: None,
            host_metadata: AgentReportHostMetadata::default(),
            persisted_task: Some(&persisted_task),
            rejected_task_transition: None,
        },
    );
    deliver_local_report_via_gateway(
        svc,
        delivery.target_tab_id,
        delivery.from_title,
        delivery.from_tab_id,
        delivery.model.clone(),
        &report,
    );
    svc.record_kpi_report(
        delivery.from_tab_id,
        Some(delivery.plan_id),
        Some(delivery.task_id),
        delivery.model.as_deref(),
        message.len(),
        &report.id,
    );
    report.id
}
