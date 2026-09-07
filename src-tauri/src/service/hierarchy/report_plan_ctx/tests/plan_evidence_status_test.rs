use super::*;

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn taskless_evidence_report_cannot_carry_a_task_transition_status() {
    let h = plan_evidence_harness();
    let token = "status-refused-e28c4d71";
    let plan_before = h.plan();
    let mut details = h.details(&h.reporter, "reporter-session", "reporter-nonce");
    details.status = Some(ReportStatus::InReview);

    let error =
        h.fx.service
            .submit_report(&h.reporter, token, details, Some(&h.plan_id), None, None)
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL));
    assert_eq!(h.plan(), plan_before);
    assert!(!h.reports().contains(token));
}
