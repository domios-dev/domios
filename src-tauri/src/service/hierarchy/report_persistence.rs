use std::collections::HashSet;
use std::path::Path;

use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::plan::{AgentRef, TaskStatus};
use crate::service::plan::batch::{BatchOp, BatchRequest};
use crate::service::plan::task_attempts::BoundWorkerReport;
use crate::service::plan::PlanStore;
use crate::service::plan_batch_authority::{
    apply_batch_with_authority, BatchAuthority, BatchWriteError,
};
use crate::service::ServiceContext;

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
) -> Result<(), String> {
    let mux = svc.mux.read();
    let mut effective_details = details.clone();
    if effective_details.caller_session_id.is_none() {
        let supplied_nonce = effective_details.session_binding_nonce.as_deref();
        let live_session = mux.get_tab(tab_id).and_then(|tab| {
            (effective_details.caller_tab_id.as_deref() == Some(tab_id)
                && tab.session_binding_nonce.as_deref() == supplied_nonce
                && tab.reported_session_binding_nonce.as_deref() == supplied_nonce)
                .then(|| tab.agent_session_id.clone())
                .flatten()
        });
        effective_details.caller_session_id = live_session;
    }
    let authority = report_authority(&effective_details, tab_id, plan_id, task_id)?;
    let authenticated = mux.get_tab(&authority.tab_id).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(&authority.session_id)
            && tab.session_binding_nonce.as_deref() == Some(&authority.session_binding_nonce)
            && tab.reported_session_binding_nonce.as_deref()
                == Some(&authority.session_binding_nonce)
    });
    if !authenticated {
        return Err(report_persistence_incident(
            tab_id,
            plan_id,
            Some(task_id),
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
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": plan_id }),
    );
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
    let (Some(plan_id), Some(super::PlanOnlyReportAuthority::EvidenceReporter(_))) =
        (plan_id, authority)
    else {
        return Ok(None);
    };
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

pub(super) fn rejected_report_delivery_message(cause: &str, message: &str) -> String {
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
) -> Result<(), String> {
    let binding = store.task_binding_for_tab(tab_id);
    if binding
        .as_ref()
        .is_some_and(|(bound_plan, bound_task)| bound_plan == plan_id && bound_task == task_id)
    {
        return persist_bound_report(
            store, tab_id, plan_id, task_id, from, message, details, now_ms,
        );
    }

    persist_explicit_report(
        store,
        plan_id,
        task_id,
        from,
        message,
        details,
        task_status,
        milestone_kind,
        now_ms,
    )
    .map_err(|cause| report_persistence_incident(tab_id, plan_id, Some(task_id), &cause))
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
) -> Result<(), String> {
    let before: HashSet<String> = store
        .get(plan_id)
        .and_then(|plan| plan.tasks.into_iter().find(|task| task.id == task_id))
        .map(|task| task.milestones.into_iter().map(|item| item.id).collect())
        .unwrap_or_default();
    let evidence = BoundWorkerReport {
        message: message.to_string(),
        status: details.status,
        branch: details.branch.clone(),
        sha: details.sha.clone(),
        changed_files: details.changed_files.clone().unwrap_or_default(),
        test_cmd: details.test_cmd.clone(),
        test_result: details.test_result.clone(),
        skipped: details.skipped.clone(),
        authority: Some(report_authority(details, tab_id, plan_id, task_id)?),
    };
    store
        .ingest_bound_worker_report(plan_id, task_id, from, evidence, now_ms)
        .map_err(|error| {
            report_persistence_incident(tab_id, plan_id, Some(task_id), &error.to_string())
        })?;

    let milestone_id = store
        .get(plan_id)
        .and_then(|plan| plan.tasks.into_iter().find(|task| task.id == task_id))
        .and_then(|task| {
            task.milestones.into_iter().find(|item| {
                !before.contains(&item.id)
                    && item.kind == "worker_report"
                    && item.from.tab_id.as_deref() == Some(tab_id)
                    && item.text.contains(message)
            })
        })
        .map(|item| item.id)
        .ok_or_else(|| {
            report_persistence_incident(
                tab_id,
                plan_id,
                Some(task_id),
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
}

#[allow(clippy::too_many_arguments)]
fn persist_explicit_report(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    from: &AgentRef,
    message: &str,
    details: &ReportDetails,
    task_status: Option<&str>,
    milestone_kind: Option<&str>,
    now_ms: u64,
) -> Result<(), String> {
    let mut ops = Vec::new();
    let blocked_review_note = details.status == Some(ReportStatus::Blocked)
        && store
            .get(plan_id)
            .and_then(|plan| {
                plan.tasks
                    .into_iter()
                    .find(|task| task.id == task_id)
                    .map(|task| task.status == TaskStatus::InReview)
            })
            .unwrap_or(false);
    if !blocked_review_note {
        if let Some(status) = task_status {
            ops.push(BatchOp::SetTaskStatus {
                client_id: None,
                task: task_id.to_string(),
                status: status.to_string(),
                status_label: (details.status == Some(ReportStatus::Blocked))
                    .then(|| message.to_string()),
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
        BatchWriteError::Plan(e) => e.to_string(),
        BatchWriteError::Refused(refusal) => refusal.message,
    })?;
    let milestone_id = result
        .milestone_ids
        .first()
        .ok_or_else(|| "the explicit plan write produced no milestone".to_string())?;
    attach_files(
        store,
        plan_id,
        task_id,
        milestone_id,
        &details.attachments,
        now_ms,
    )
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
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::plan::NewTask;
    use crate::service::test_fixture::ServiceFixture;

    fn add_task(store: &PlanStore, plan_id: &str, name: &str, now_ms: u64) -> String {
        store
            .add_task(
                plan_id,
                NewTask {
                    name: name.into(),
                    kind: "implement".into(),
                    acceptance_criteria: vec!["report persists".into()],
                    test_plan: Some("focused regression".into()),
                    ..Default::default()
                },
                now_ms,
            )
            .unwrap()
    }

    #[test]
    fn late_bound_no_id_report_persists_exact_task_milestone() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "late binding".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = add_task(&store, &plan.id, "fresh dispatch", 2);
        assert_eq!(store.task_binding_for_tab("worker"), None);
        store
            .assign_task(&plan.id, &task_id, "worker", None, 3)
            .unwrap();
        store
            .update_attempt_executor_identity(
                &plan.id,
                &task_id,
                "worker",
                None,
                Some("worker-session"),
                Some("worker-nonce"),
                4,
            )
            .unwrap();
        // A bound report carries the caller's own authoritative identity; the
        // refusal below is the product incident, not a default to fill in.
        let details = ReportDetails {
            caller_tab_id: Some("worker".into()),
            caller_session_id: Some("worker-session".into()),
            session_binding_nonce: Some("worker-nonce".into()),
            owner_epoch: Some(0),
            ..Default::default()
        };

        persist_local_task_report(
            &store,
            "worker",
            &plan.id,
            &task_id,
            &AgentRef::from_tab("worker"),
            "LATE_BIND_MARKER",
            &details,
            None,
            None,
            5,
        )
        .unwrap();

        let saved = store.get(&plan.id).unwrap();
        let task = saved.tasks.iter().find(|task| task.id == task_id).unwrap();
        assert_eq!(task.milestones.len(), 1);
        assert_eq!(task.milestones[0].kind, "worker_report");
        assert_eq!(task.milestones[0].text, "LATE_BIND_MARKER");
        assert_eq!(task.milestones[0].from.tab_id.as_deref(), Some("worker"));
    }

    #[test]
    fn explicit_target_wins_over_discovered_binding() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut bound_plan = Plan::new(
            "bound".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-a"),
        );
        store.save(&mut bound_plan, 1).unwrap();
        let bound_task = add_task(&store, &bound_plan.id, "bound task", 2);
        store
            .assign_task(&bound_plan.id, &bound_task, "worker", None, 3)
            .unwrap();

        let mut explicit_plan = Plan::new(
            "explicit".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-b"),
        );
        store.save(&mut explicit_plan, 4).unwrap();
        let explicit_task = add_task(&store, &explicit_plan.id, "explicit task", 5);

        persist_local_task_report(
            &store,
            "worker",
            &explicit_plan.id,
            &explicit_task,
            &AgentRef::from_tab("worker"),
            "EXPLICIT_MARKER",
            &ReportDetails::default(),
            None,
            None,
            6,
        )
        .unwrap();

        let bound = store.get(&bound_plan.id).unwrap();
        assert!(bound.tasks[0].milestones.is_empty());
        let explicit = store.get(&explicit_plan.id).unwrap();
        assert_eq!(explicit.tasks[0].milestones[0].text, "EXPLICIT_MARKER");
    }

    #[test]
    fn blocked_explicit_report_sets_status_and_reason() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "explicit blocked report".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = add_task(&store, &plan.id, "blocked task", 2);
        let reason = "explicit worker report reason";
        let details = ReportDetails {
            status: Some(ReportStatus::Blocked),
            ..Default::default()
        };

        persist_local_task_report(
            &store,
            "worker",
            &plan.id,
            &task_id,
            &AgentRef::from_tab("worker"),
            reason,
            &details,
            Some("blocked"),
            Some("worker_report"),
            3,
        )
        .unwrap();

        let task = store.get(&plan.id).unwrap().tasks.remove(0);
        assert_eq!(task.status, TaskStatus::Blocked);
        assert_eq!(task.status_label.as_deref(), Some(reason));
        assert_eq!(task.milestones.len(), 1);
    }

    #[test]
    fn blocked_explicit_report_on_in_review_task_is_kept_as_a_milestone() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "explicit blocked review note".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = add_task(&store, &plan.id, "reviewed task", 2);
        store
            .with_plan(&plan.id, 3, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .unwrap();
                task.status = TaskStatus::InReview;
                Ok(())
            })
            .unwrap();

        let details = ReportDetails {
            status: Some(ReportStatus::Blocked),
            ..Default::default()
        };
        persist_local_task_report(
            &store,
            "worker",
            &plan.id,
            &task_id,
            &AgentRef::from_tab("worker"),
            "review still owns this task",
            &details,
            Some("blocked"),
            Some("worker_report"),
            4,
        )
        .unwrap();

        let task = store.get(&plan.id).unwrap().tasks.remove(0);
        assert_eq!(task.status, TaskStatus::InReview);
        assert_eq!(task.milestones.len(), 1);
        assert_eq!(
            task.milestones[0].text,
            "BLOCKED: review still owns this task"
        );
    }

    #[test]
    fn missing_task_persistence_is_a_loud_incident_not_parent_only_success() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "persistence guard".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = add_task(&store, &plan.id, "guarded task", 2);
        store
            .assign_task(&plan.id, &task_id, "worker", None, 3)
            .unwrap();

        let error = persist_local_task_report(
            &store,
            "worker",
            &plan.id,
            &task_id,
            &AgentRef::from_tab("wrong-reporter"),
            "MUST_NOT_DISAPPEAR",
            &ReportDetails::default(),
            None,
            None,
            4,
        )
        .unwrap_err();

        assert!(error.contains("ORCHESTRATION PRODUCT INCIDENT"));
        assert!(error.contains("parent delivery alone cannot prove"));
        assert!(error.contains("no task-report success was returned"));
        assert!(store.get(&plan.id).unwrap().tasks[0].milestones.is_empty());
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn spoofed_or_rebound_report_rejects_transition_without_mutating_task() {
        let (mux, _) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let workspace = fixture
            .service
            .mux
            .write()
            .create_workspace("reports".into(), None)
            .id;
        let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("new-session".into());
            tab.session_binding_nonce = Some("new-nonce".into());
            tab.reported_session_binding_nonce = Some("new-nonce".into());
        }
        let mut plan = Plan::new(
            "reports".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = add_task(&fixture.service.plan_store, &plan.id, "report", 2);
        let details = ReportDetails {
            caller_tab_id: Some(tab_id.clone()),
            caller_session_id: Some("old-session".into()),
            session_binding_nonce: Some("old-nonce".into()),
            owner_epoch: Some(1),
            ..Default::default()
        };
        let error = persist_authorized_local_task_report(
            &fixture.service,
            &tab_id,
            &plan.id,
            &task_id,
            &AgentRef::from_tab(&tab_id),
            "must still deliver outside this persistence seam",
            &details,
            None,
            None,
            3,
        )
        .unwrap_err();
        assert!(error.contains("stale, spoofed, or rebound"));
        assert!(fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .milestones
            .is_empty());
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn verified_nonce_resolves_missing_report_session_id() {
        let (mux, _) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let workspace = fixture
            .service
            .mux
            .write()
            .create_workspace("reports".into(), None)
            .id;
        let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("live-session".into());
            tab.session_binding_nonce = Some("live-nonce".into());
            tab.reported_session_binding_nonce = Some("live-nonce".into());
        }
        let mut plan = Plan::new(
            "reports".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = add_task(&fixture.service.plan_store, &plan.id, "report", 2);
        fixture
            .service
            .plan_store
            .assign_task(&plan.id, &task_id, &tab_id, None, 3)
            .unwrap();
        fixture
            .service
            .plan_store
            .update_attempt_executor_identity(
                &plan.id,
                &task_id,
                &tab_id,
                None,
                Some("live-session"),
                Some("live-nonce"),
                4,
            )
            .unwrap();
        let details = ReportDetails {
            caller_tab_id: Some(tab_id.clone()),
            caller_session_id: None,
            session_binding_nonce: Some("live-nonce".into()),
            owner_epoch: Some(0),
            ..Default::default()
        };
        persist_authorized_local_task_report(
            &fixture.service,
            &tab_id,
            &plan.id,
            &task_id,
            &AgentRef::from_tab(&tab_id),
            "report persisted",
            &details,
            None,
            None,
            5,
        )
        .unwrap();
        assert_eq!(
            fixture.service.plan_store.get(&plan.id).unwrap().tasks[0].milestones[0].text,
            "report persisted"
        );
    }
}
