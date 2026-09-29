use super::*;

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn owner_rebound_invalidates_its_prior_reporter_grants() {
    let h = plan_evidence_harness();
    let token = "owner-rebound-refused-a57d143c";
    let plan_before = h.plan();
    {
        let mut mux = h.fx.service.mux.write();
        let owner = mux.get_tab_mut(&h.root).unwrap();
        owner.agent_session_id = Some("rebound-owner-session".into());
        owner.session_binding_nonce = Some("rebound-owner-nonce".into());
        owner.reported_session_binding_nonce = Some("rebound-owner-nonce".into());
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
    assert_eq!(h.plan(), plan_before);
    assert!(!h.reports().contains(token));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn team_root_orchestrator_report_persists_as_plan_stride_without_parent() {
    let h = plan_evidence_harness();
    let token = "root-report-stride-proof";

    let result = h
        .fx
        .service
        .submit_report(
            &h.root,
            token,
            h.details(&h.root, "owner-session", "owner-nonce"),
            Some(&h.plan_id),
            None,
            None,
        )
        .unwrap();

    assert!(result.persisted_task.is_none());
    assert!(result.persisted_plan_evidence.is_some());
    assert!(h.plan().strides.iter().any(|stride| stride.text == token));
}
