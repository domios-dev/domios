use super::*;

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plan_evidence_resolves_absent_session_from_attested_nonce() {
    let h = plan_evidence_harness();
    let before = h.plan().tasks;
    let token = "nonce-attested-evidence-7af43";
    let mut details = h.details(&h.reporter, "reporter-session", "reporter-nonce");
    details.caller_session_id = None;

    let result =
        h.fx.service
            .submit_report(&h.reporter, token, details, Some(&h.plan_id), None, None)
            .unwrap();

    assert!(result.persisted_task.is_none());
    assert!(result.persisted_plan_evidence.is_some());
    let plan = h.plan();
    let stride = plan
        .strides
        .iter()
        .find(|stride| stride.text == token)
        .unwrap();
    assert_eq!(stride.from.session_id.as_deref(), Some("reporter-session"));
    assert_eq!(
        serde_json::to_value(plan.tasks).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert!(h.reports().contains(token));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plan_evidence_absent_session_still_requires_nonce_and_grant() {
    for case in [
        "wrong_nonce",
        "missing_nonce",
        "wrong_session",
        "unreported",
        "no_grant",
        "rebound",
    ] {
        let h = plan_evidence_harness();
        let target = if case == "no_grant" {
            &h.ordinary
        } else {
            &h.reporter
        };
        let nonce = if case == "no_grant" {
            "ordinary-nonce"
        } else {
            "reporter-nonce"
        };
        let mut details = h.details(target, "reporter-session", nonce);
        details.caller_session_id = None;
        match case {
            "wrong_nonce" => details.session_binding_nonce = Some("foreign-nonce".into()),
            "missing_nonce" => details.session_binding_nonce = None,
            "wrong_session" => details.caller_session_id = Some("foreign-session".into()),
            "unreported" => {
                h.fx.service
                    .mux
                    .write()
                    .get_tab_mut(target)
                    .unwrap()
                    .reported_session_binding_nonce = None
            }
            "rebound" => {
                let mut mux = h.fx.service.mux.write();
                let tab = mux.get_tab_mut(target).unwrap();
                tab.agent_session_id = Some("new-session".into());
                tab.session_binding_nonce = Some("new-nonce".into());
                tab.reported_session_binding_nonce = Some("new-nonce".into());
                details.session_binding_nonce = Some("new-nonce".into());
            }
            _ => {}
        }
        let before = serde_json::to_value(h.plan()).unwrap();
        let error =
            h.fx.service
                .submit_report(target, case, details, Some(&h.plan_id), None, None)
                .unwrap_err();
        assert!(
            error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL),
            "{case}: {error}"
        );
        assert_eq!(serde_json::to_value(h.plan()).unwrap(), before, "{case}");
        assert!(!h.reports().contains(case), "{case}");
        if case == "no_grant" {
            assert!(error.contains("no current plan evidence grant"), "{error}");
        }
    }
}
