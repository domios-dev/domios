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
