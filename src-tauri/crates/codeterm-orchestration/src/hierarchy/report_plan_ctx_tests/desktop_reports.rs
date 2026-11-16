use super::*;

#[test]
fn authorized_taskless_reporter_persists_plan_evidence_and_reaches_owner_without_task_mutation() {
    let h = plan_evidence_harness();
    let task_before = h
        .plan()
        .tasks
        .into_iter()
        .find(|task| task.id == h.task_id)
        .unwrap();
    let token = "qa-report-7fb62d8b";

    let result =
        h.fx.service
            .submit_report(
                &h.reporter,
                token,
                h.details(&h.reporter, "reporter-session", "reporter-nonce"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap();

    assert!(result.persisted_task.is_none());
    assert!(result.rejected_task_transition.is_none());
    assert!(result.persisted_plan_evidence.is_some());
    assert_eq!(
        result.report_outcome,
        Some(ReportOutcomeClass::StructurallyNotAttachable)
    );
    let plan = h.plan();
    let evidence = plan
        .strides
        .iter()
        .find(|stride| stride.text == token)
        .unwrap();
    assert_eq!(evidence.kind, "plan_evidence_report");
    assert_eq!(evidence.from.tab_id.as_deref(), Some(h.reporter.as_str()));
    assert_eq!(
        evidence.from.session_id.as_deref(),
        Some("reporter-session")
    );
    let task_after = plan.tasks.iter().find(|task| task.id == h.task_id).unwrap();
    assert_eq!(task_after.status, task_before.status);
    assert_eq!(task_after.assigned_tab_id, task_before.assigned_tab_id);
    assert_eq!(task_after.milestones, task_before.milestones);
    assert!(h.reports().contains(token));
}

#[test]
fn ordinary_taskless_tab_is_refused_without_plan_or_delivery_mutation() {
    let h = plan_evidence_harness();
    let token = "ordinary-refused-36d4b0f8";
    let strides_before = h.plan().strides;

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
    assert_eq!(h.plan().strides, strides_before);
    assert!(!h.reports().contains(token));
}

#[test]
fn evidence_grant_is_plan_scoped_and_cannot_write_a_foreign_plan() {
    let h = plan_evidence_harness();
    let mut foreign = Plan::new(
        "foreign evidence plan".into(),
        h.fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&h.root),
    );
    foreign.team_id = Some("team-foreign-evidence".into());
    h.fx.service.plan_store.save(&mut foreign, 20).unwrap();
    let token = "foreign-plan-refused-2f5ec639";
    let original_before = h.plan().strides;
    let foreign_before = foreign.strides.clone();
    let mut details = h.details(&h.reporter, "reporter-session", "reporter-nonce");
    details.plan_id = Some(foreign.id.clone());

    let error =
        h.fx.service
            .submit_report(&h.reporter, token, details, Some(&foreign.id), None, None)
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    assert_eq!(h.plan().strides, original_before);
    assert_eq!(
        h.fx.service.plan_store.get(&foreign.id).unwrap().strides,
        foreign_before
    );
    assert!(!h.reports().contains(token));
}

#[test]
fn rebound_authorized_reporter_is_refused_without_plan_or_delivery_mutation() {
    let h = plan_evidence_harness();
    let token = "rebound-refused-f5c9d7a1";
    let strides_before = h.plan().strides;
    {
        let mut mux = h.fx.service.mux.write();
        let tab = mux.get_tab_mut(&h.reporter).unwrap();
        tab.agent_session_id = Some("rebound-session".into());
        tab.session_binding_nonce = Some("rebound-nonce".into());
        tab.reported_session_binding_nonce = Some("rebound-nonce".into());
    }

    let error =
        h.fx.service
            .submit_report(
                &h.reporter,
                token,
                h.details(&h.reporter, "reporter-session", "reporter-nonce"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    assert_eq!(h.plan().strides, strides_before);
    assert!(!h.reports().contains(token));
}

#[test]
fn task_bound_reporter_cannot_use_plan_evidence_capability() {
    let h = plan_evidence_harness();
    let token = "task-bound-refused-3cf68e0a";
    let plan_before = h.plan();

    let error =
        h.fx.service
            .submit_report(
                &h.bound,
                token,
                h.details(&h.bound, "bound-session", "bound-nonce"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    let plan_after = h.plan();
    assert_eq!(plan_after.strides, plan_before.strides);
    assert_eq!(plan_after.tasks, plan_before.tasks);
    assert!(!h.reports().contains(token));
}
