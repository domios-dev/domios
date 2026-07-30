use std::collections::HashSet;
use std::path::Path;

use crate::agent::reports::ReportDetails;
use crate::plan::AgentRef;
use crate::service::plan::batch::{BatchOp, BatchRequest};
use crate::service::plan::task_attempts::BoundWorkerReport;
use crate::service::plan::PlanStore;

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
    if let Some(status) = task_status {
        ops.push(BatchOp::SetTaskStatus {
            client_id: None,
            task: task_id.to_string(),
            status: status.to_string(),
        });
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
    let result = store
        .apply_batch(plan_id, BatchRequest { key: None, ops }, now_ms)
        .map_err(|error| error.to_string())?;
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
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::plan::NewTask;

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

        persist_local_task_report(
            &store,
            "worker",
            &plan.id,
            &task_id,
            &AgentRef::from_tab("worker"),
            "LATE_BIND_MARKER",
            &ReportDetails::default(),
            None,
            None,
            4,
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
}
