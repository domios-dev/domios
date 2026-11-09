use super::*;
use crate::plan::{OrchestratorRef, Plan};

#[test]
fn planner_report_session_completion_preserves_registration_and_refuses_foreign_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "planner".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: None,
        title: Some("Euclid".into()),
    });
    store.save(&mut plan, 1).unwrap();
    let before = store.get(&plan.id).unwrap();
    assert!(store
        .complete_planner_report_session(&plan.id, "foreign", "session", 2)
        .is_err());
    assert_eq!(store.get(&plan.id).unwrap(), before);
    store
        .complete_planner_report_session(&plan.id, "planner", "session", 3)
        .unwrap();
    let after = store.get(&plan.id).unwrap();
    assert_eq!(
        after.planner,
        Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: Some("session".into()),
            title: Some("Euclid".into())
        })
    );
    store
        .complete_planner_report_session(&plan.id, "planner", "session", 4)
        .unwrap();
    assert_eq!(store.get(&plan.id).unwrap(), after);
    assert!(store
        .complete_planner_report_session(&plan.id, "planner", "foreign-session", 5)
        .is_err());
    assert_eq!(store.get(&plan.id).unwrap(), after);
}
