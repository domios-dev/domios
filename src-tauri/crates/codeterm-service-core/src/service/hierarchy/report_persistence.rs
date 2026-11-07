use std::path::Path;

use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::plan::AgentRef;
#[cfg(test)]
use crate::plan::TaskStatus;
use crate::service::plan::batch::{BatchOp, BatchRequest};
use crate::service::plan::task_attempts::BoundWorkerReport;
use crate::service::plan::PlanStore;
use crate::service::plan_batch_authority::{
    apply_batch_with_authority, BatchAuthority, BatchWriteError,
};
use crate::service::ServiceContext;
use codeterm_types::agent::ReportOutcomeClass;

const SHARED_REPORT_NEXT_STEP: &str = "commit only your declared paths as one \
     ordinary commit on the current shared checkout HEAD and report again with that literal \
     SHA; if the work cannot land on the shared HEAD, ask the Orchestrator for a lane.";

#[derive(Debug)]
pub(super) struct ReportPersistenceFailure {
    pub cause: String,
    pub report_outcome: ReportOutcomeClass,
}

pub(super) fn report_persistence_incident(
    tab_id: &str,
    plan_id: &str,
    task_id: Option<&str>,
    cause: &str,
) -> String {
    let target = task_id
        .map(|task| format!("{plan_id}/{task}"))
        .unwrap_or_else(|| format!("{plan_id}/<unknown>"));
    format!(
        "ORCHESTRATION PRODUCT INCIDENT: Worker tab '{tab_id}' report was not persisted to \
         authoritative task '{target}'.\nImpact: parent delivery alone cannot prove the task \
         received this outcome.\nFallback: pass explicit --plan and --task and alert the \
         Orchestrator; no task-report success was returned.\nCause: {cause}"
    )
}

fn incident_failure(
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    cause: &str,
) -> ReportPersistenceFailure {
    stale_failure(report_persistence_incident(
        tab_id,
        plan_id,
        Some(task_id),
        cause,
    ))
}

pub(super) fn stale_failure(cause: String) -> ReportPersistenceFailure {
    ReportPersistenceFailure {
        cause,
        report_outcome: ReportOutcomeClass::StaleBindingConflict,
    }
}

fn plan_failure(
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    error: crate::service::plan::PlanError,
) -> ReportPersistenceFailure {
    match error {
        crate::service::plan::PlanError::ReviewEvidenceRequired => ReportPersistenceFailure {
            cause: crate::service::plan::PlanError::ReviewEvidenceRequired.to_string(),
            report_outcome: ReportOutcomeClass::MissingReviewEvidence,
        },
        crate::service::plan::PlanError::SharedReportRejected(cause) => ReportPersistenceFailure {
            cause: format!("{cause}\nWorker next step: {SHARED_REPORT_NEXT_STEP}"),
            report_outcome: ReportOutcomeClass::TransitionRejected,
        },
        other => incident_failure(tab_id, plan_id, task_id, &other.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_authorized_local_task_report(
    svc: &ServiceContext,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    from: &AgentRef,
    message: &str,
    details: &ReportDetails,
    task_status: Option<&str>,
    milestone_kind: Option<&str>,
    now_ms: u64,
) -> Result<(), ReportPersistenceFailure> {
    let mux = svc.mux.read();
    let mut effective_details = details.clone();
    if effective_details.caller_session_id.is_none() {
        let live_identity = mux.get_tab(tab_id).and_then(|tab| {
            resolve_missing_report_identity(
                &effective_details,
                tab_id,
                tab.agent_session_id.as_deref(),
                tab.session_binding_nonce.as_deref(),
                tab.reported_session_binding_nonce.as_deref(),
            )
        });
        if let Some((session_id, nonce)) = live_identity {
            effective_details.caller_session_id = Some(session_id);
            effective_details.session_binding_nonce = Some(nonce);
        }
    }
    let authority =
        report_authority(&effective_details, tab_id, plan_id, task_id).map_err(stale_failure)?;
    let authenticated = mux.get_tab(&authority.tab_id).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(&authority.session_id)
            && tab.session_binding_nonce.as_deref() == Some(&authority.session_binding_nonce)
            && tab.reported_session_binding_nonce.as_deref()
                == Some(&authority.session_binding_nonce)
    });
    if !authenticated {
        return Err(incident_failure(
            tab_id,
            plan_id,
            task_id,
            "report caller binding is stale, spoofed, or rebound",
        ));
    }
    drop(mux);
    persist_local_task_report(
        &svc.plan_store,
        tab_id,
        plan_id,
        task_id,
        from,
        message,
        &effective_details,
        task_status,
        milestone_kind,
        now_ms,
    )
}

fn resolve_missing_report_identity(
    details: &ReportDetails,
    tab_id: &str,
    session_id: Option<&str>,
    configured_nonce: Option<&str>,
    reported_nonce: Option<&str>,
) -> Option<(String, String)> {
    if details.caller_tab_id.as_deref() != Some(tab_id) || details.caller_session_id.is_some() {
        return None;
    }
    let session_id = session_id.filter(|session| !session.trim().is_empty())?;
    let nonce = configured_nonce.filter(|nonce| !nonce.trim().is_empty())?;
    (reported_nonce == Some(nonce)
        && details
            .session_binding_nonce
            .as_deref()
            .is_none_or(|supplied| supplied == nonce))
    .then(|| (session_id.to_string(), nonce.to_string()))
}

fn persist_authorized_plan_evidence_report(
    svc: &ServiceContext,
    tab_id: &str,
    plan_id: &str,
    from: &AgentRef,
    message: &str,
    now_ms: u64,
) -> Result<String, crate::service::ReportSubmissionError> {
    let stride_id = svc
        .plan_store
        .add_plan_evidence_report(plan_id, message, from.clone(), now_ms)
        .map_err(crate::service::ReportSubmissionError::from)?;
    svc.plan_turn_counter.reset_activity(tab_id);
    svc.emit_committed_plan_changed(&plan_id);
    Ok(stride_id)
}

pub(super) fn persist_plan_only_evidence(
    svc: &ServiceContext,
    authority: Option<super::PlanOnlyReportAuthority>,
    tab_id: &str,
    plan_id: Option<&str>,
    from: &AgentRef,
    message: &str,
    now_ms: u64,
) -> Result<Option<(String, String)>, crate::service::ReportSubmissionError> {
    let Some(plan_id) = plan_id else {
        return Ok(None);
    };
    if !matches!(
        authority,
        Some(super::PlanOnlyReportAuthority::EvidenceReporter(_))
            | Some(super::PlanOnlyReportAuthority::PlanOwner)
    ) {
        return Ok(None);
    }
    let stride_id =
        persist_authorized_plan_evidence_report(svc, tab_id, plan_id, from, message, now_ms)?;
    Ok(Some((plan_id.to_string(), stride_id)))
}

fn report_authority(
    details: &ReportDetails,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
) -> Result<crate::service::plan::interruption::TaskMutationAuthority, String> {
    let authority = crate::service::plan::interruption::TaskMutationAuthority {
        tab_id: details.caller_tab_id.clone().ok_or_else(|| {
            report_persistence_incident(tab_id, plan_id, Some(task_id), "missing caller tab id")
        })?,
        session_id: details.caller_session_id.clone().ok_or_else(|| {
            report_persistence_incident(tab_id, plan_id, Some(task_id), "missing caller session id")
        })?,
        session_binding_nonce: details.session_binding_nonce.clone().ok_or_else(|| {
            report_persistence_incident(
                tab_id,
                plan_id,
                Some(task_id),
                "missing caller binding nonce",
            )
        })?,
        owner_epoch: details.owner_epoch.ok_or_else(|| {
            report_persistence_incident(
                tab_id,
                plan_id,
                Some(task_id),
                "missing claimed owner epoch",
            )
        })?,
    };
    if authority.tab_id != tab_id {
        return Err(report_persistence_incident(
            tab_id,
            plan_id,
            Some(task_id),
            "report caller tab id is spoofed",
        ));
    }
    Ok(authority)
}

pub(super) fn rejected_report_delivery_message(
    outcome: ReportOutcomeClass,
    cause: &str,
    message: &str,
) -> String {
    if outcome == ReportOutcomeClass::MissingReviewEvidence {
        return format!(
            "REPORT RECEIVED — TASK TRANSITION REJECTED\n\
             The report was not persisted. Ask the Orchestrator to add exact acceptance and QA evidence, then resubmit with --status in_review; do not retry without a status.\n\
             Required evidence: {cause}\n\n{message}"
        );
    }
    if outcome == ReportOutcomeClass::TransitionRejected {
        return format!(
            "REPORT RECEIVED — TASK TRANSITION REJECTED\n\
             The report was not persisted and the task is not in review; do not accept or bounce it. The Worker corrects and reports again.\n\
             Cause: {cause}\n\n{message}"
        );
    }
    format!(
        "REPORT RECEIVED — TASK TRANSITION REJECTED\n\
         The report itself was delivered and must not be resent.\n\
         Owner action required: {cause}\n\n{message}"
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_local_task_report(
    store: &PlanStore,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    from: &AgentRef,
    message: &str,
    details: &ReportDetails,
    task_status: Option<&str>,
    milestone_kind: Option<&str>,
    now_ms: u64,
) -> Result<(), ReportPersistenceFailure> {
    let binding = store
        .assigned_task_binding_projection(tab_id)
        .map_err(|error| plan_failure(tab_id, plan_id, task_id, error))?;
    if binding
        .as_ref()
        .is_some_and(|binding| binding.plan_id == plan_id && binding.task_id == task_id)
    {
        return persist_bound_report(
            store, tab_id, plan_id, task_id, from, message, details, now_ms,
        );
    }

    persist_explicit_report(
        store,
        tab_id,
        plan_id,
        task_id,
        from,
        message,
        details,
        task_status,
        milestone_kind,
        now_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_bound_report(
    store: &PlanStore,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    from: &AgentRef,
    message: &str,
    details: &ReportDetails,
    now_ms: u64,
) -> Result<(), ReportPersistenceFailure> {
    let evidence = BoundWorkerReport {
        message: message.to_string(),
        status: details.status,
        branch: details.branch.clone(),
        sha: details.sha.clone(),
        changed_files: details.changed_files.clone().unwrap_or_default(),
        test_cmd: details.test_cmd.clone(),
        test_result: details.test_result.clone(),
        skipped: details.skipped.clone(),
        authority: Some(
            report_authority(details, tab_id, plan_id, task_id).map_err(stale_failure)?,
        ),
    };
    let receipt = store
        .ingest_bound_worker_report(plan_id, task_id, from, evidence, now_ms)
        .map_err(|error| plan_failure(tab_id, plan_id, task_id, error))?;
    let milestone_id = receipt.milestone_id.ok_or_else(|| {
        incident_failure(
            tab_id,
            plan_id,
            task_id,
            "the plan write produced no matching worker_report milestone",
        )
    })?;
    attach_files(
        store,
        plan_id,
        task_id,
        &milestone_id,
        &details.attachments,
        now_ms,
    )
    .map_err(|cause| incident_failure(tab_id, plan_id, task_id, &cause))
}

#[allow(clippy::too_many_arguments)]
fn persist_explicit_report(
    store: &PlanStore,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    from: &AgentRef,
    message: &str,
    details: &ReportDetails,
    task_status: Option<&str>,
    milestone_kind: Option<&str>,
    now_ms: u64,
) -> Result<(), ReportPersistenceFailure> {
    let mut ops = Vec::new();
    if details.status != Some(ReportStatus::Blocked) {
        if let Some(status) = task_status {
            ops.push(BatchOp::SetTaskStatus {
                client_id: None,
                task: task_id.to_string(),
                status: status.to_string(),
                status_label: None,
            });
        }
    }
    ops.push(BatchOp::AddMilestone {
        client_id: Some("report".into()),
        task: task_id.to_string(),
        kind: milestone_kind.unwrap_or("status_update").into(),
        text: super::milestone_text(message, details.status),
        from: Some(from.clone()),
        key: None,
        links: vec![],
    });
    // Declared, never exempt by omission: this path authenticated the caller tab,
    // session and binding nonce, and resolved the task hold, before composing ops.
    let result = apply_batch_with_authority(
        store,
        plan_id,
        BatchRequest { key: None, ops },
        now_ms,
        BatchAuthority::EstablishedUpstream {
            established_by: "agent.report: caller binding authenticated and task hold resolved",
        },
    )
    .map_err(|error| match error {
        BatchWriteError::Plan(error) => plan_failure(tab_id, plan_id, task_id, error),
        BatchWriteError::Refused(refusal) => {
            incident_failure(tab_id, plan_id, task_id, &refusal.message)
        }
    })?;
    let milestone_id = result.milestone_ids.first().ok_or_else(|| {
        incident_failure(
            tab_id,
            plan_id,
            task_id,
            "the explicit plan write produced no milestone",
        )
    })?;
    attach_files(
        store,
        plan_id,
        task_id,
        milestone_id,
        &details.attachments,
        now_ms,
    )
    .map_err(|cause| incident_failure(tab_id, plan_id, task_id, &cause))
}

fn attach_files(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    milestone_id: &str,
    attachments: &[String],
    now_ms: u64,
) -> Result<(), String> {
    for path in attachments {
        store
            .attach_to_milestone(plan_id, task_id, milestone_id, Path::new(path), now_ms)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "report_persistence/tests.rs"]
mod tests;
