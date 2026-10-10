use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn planner_shaped_tab_without_capability_is_refused() {
    let h = planner_report_harness();
    {
        let mut mux = h.fx.service.mux.write();
        let tab = mux.get_tab_mut(&h.foreign_tab).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("planner".into());
    }

    let error =
        h.fx.service
            .submit_report(
                &h.foreign_tab,
                "planner-shaped-refused-9d8643e1",
                h.details(&h.foreign_tab, "foreign"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    assert!(!h.reports().contains("planner-shaped-refused-9d8643e1"));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn planned_team_taskless_worker_reports_to_parent_without_plan_write() {
    let h = plan_evidence_harness();
    let token = "worker-parent-message-452bf1e0";
    let plan_before = h.plan();
    {
        let mut mux = h.fx.service.mux.write();
        let worker = mux.get_tab_mut(&h.ordinary).unwrap();
        worker.role = Some(AgentRole::Worker);
        worker.team_id = Some("team-evidence".into());
    }

    let result =
        h.fx.service
            .submit_report(
                &h.ordinary,
                token,
                h.details(&h.ordinary, "ordinary-session", "ordinary-nonce"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap();

    assert_eq!(result.orchestrator_tab_id, h.root);
    assert!(result.persisted_task.is_none());
    assert!(result.persisted_plan_evidence.is_none());
    assert!(result.rejected_task_transition.is_none());
    assert_eq!(
        result.report_outcome,
        Some(ReportOutcomeClass::StructurallyNotAttachable)
    );
    assert_eq!(h.plan(), plan_before);
    assert!(h.reports().contains(token));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn planned_team_worker_with_foreign_parent_cannot_use_parent_message_path() {
    let h = plan_evidence_harness();
    let token = "worker-foreign-parent-refused-47c3586a";
    {
        let mut mux = h.fx.service.mux.write();
        let worker = mux.get_tab_mut(&h.ordinary).unwrap();
        worker.role = Some(AgentRole::Worker);
        worker.team_id = Some("team-evidence".into());
        worker.parent_tab_id = Some(h.reporter.clone());
    }
    let plan_before = h.plan();

    let error =
        h.fx.service
            .submit_report(
                &h.ordinary,
                token,
                h.details(&h.ordinary, "ordinary-session", "ordinary-nonce"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    assert_eq!(h.plan(), plan_before);
    assert!(!h.reports().contains(token));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn planned_team_taskless_worker_parent_report_cannot_transition_a_task() {
    let h = plan_evidence_harness();
    let token = "worker-status-refused-407af69b";
    let plan_before = h.plan();
    {
        let mut mux = h.fx.service.mux.write();
        let worker = mux.get_tab_mut(&h.ordinary).unwrap();
        worker.role = Some(AgentRole::Worker);
        worker.team_id = Some("team-evidence".into());
    }
    let mut details = h.details(&h.ordinary, "ordinary-session", "ordinary-nonce");
    details.status = Some(ReportStatus::InReview);

    let error =
        h.fx.service
            .submit_report(&h.ordinary, token, details, Some(&h.plan_id), None, None)
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    assert_eq!(h.plan(), plan_before);
    assert!(!h.reports().contains(token));
}
