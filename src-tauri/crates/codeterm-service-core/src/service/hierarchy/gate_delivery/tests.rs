use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::plan::PlannerGateRead;

#[test]
fn planner_gate_routes_to_the_plan_owner() {
    let mut read = PlannerGateRead {
        owner: OrchestratorRef::from_tab("owner"),
        planner: Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: Some("session".into()),
            title: None,
        }),
        request: None,
        delivery: None,
    };
    assert_eq!(read.owner_for_gate(Some("planner")), Some("owner"));
    assert_eq!(read.owner_for_gate(Some("worker")), None);
    assert_eq!(read.owner_for_gate(None), None);
    read.owner.tab_id = None;
    assert_eq!(read.owner_for_gate(Some("planner")), None);
}
