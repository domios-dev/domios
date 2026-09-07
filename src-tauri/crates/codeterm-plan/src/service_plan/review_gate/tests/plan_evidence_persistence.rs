use super::*;

#[test]
fn plan_evidence_grant_and_report_survive_store_reopen() {
    let (dir, store, plan_id) = setup();
    let from = AgentRef {
        tab_id: Some("owner-tab".into()),
        session_id: Some("owner-session".into()),
        title: None,
    };
    store
        .grant_plan_evidence_reporter(
            &plan_id,
            "qa-tab",
            "qa-session",
            "qa-binding-nonce",
            PlanEvidenceClass::CanonicalQa,
            from.clone(),
            2,
        )
        .unwrap();
    let token = "durable-evidence-c84a106f";
    store
        .add_plan_evidence_report(&plan_id, token, AgentRef::from_tab("qa-tab"), 3)
        .unwrap();
    drop(store);

    let reopened = PlanStore::new_for_test(dir.path());
    let plan = reopened.get(&plan_id).unwrap();
    assert!(plan.evidence_reporters.iter().any(|grant| {
        grant.tab_id == "qa-tab"
            && grant.session_id == "qa-session"
            && grant.session_binding_digest == plan_evidence_binding_digest("qa-binding-nonce")
            && grant.class == PlanEvidenceClass::CanonicalQa
            && grant.granted_by == from
    }));
    assert!(plan.strides.iter().any(|stride| {
        stride.kind == PLAN_EVIDENCE_REPORT_MILESTONE_KIND && stride.text == token
    }));
    assert!(!serde_json::to_string(&plan)
        .unwrap()
        .contains("qa-binding-nonce"));
}
