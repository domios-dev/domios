use super::*;
use serde_json::json;

fn record(id: &str, parent: Option<&str>, session: Option<&str>) -> ClosedTab {
    serde_json::from_value(json!({
        "tab_id": id,
        "parent_tab_id": parent,
        "title": id,
        "agent_type": "provider",
        "agent_session_id": session,
        "closed_at": 123,
    }))
    .unwrap()
}

#[test]
fn plan_preserves_records_and_each_exact_session() {
    let tabs = [
        record("worker", Some("manager"), Some("session-worker")),
        record("manager", None, Some("session-manager")),
        record("unknown", None, None),
    ];
    let plan = GroupRestorePlan::new(&tabs).unwrap();
    assert_eq!(plan.tabs, &tabs);
    assert_eq!(plan.parent_indices, vec![Some(1), None, None]);
    assert_eq!(plan.active_index, 1);
    assert_eq!(
        plan.tabs[0].agent_session_id.as_deref(),
        Some("session-worker")
    );
    assert_eq!(
        plan.tabs[1].agent_session_id.as_deref(),
        Some("session-manager")
    );
    assert_eq!(plan.tabs[2].agent_session_id, None);
}

#[test]
fn plan_detaches_unrestored_parents() {
    let tabs = [record("child", Some("outside-group"), Some("own-session"))];
    let plan = GroupRestorePlan::new(&tabs).unwrap();
    assert_eq!(plan.parent_indices, vec![None]);
    assert_eq!(plan.active_index, 0);
}

#[test]
fn plan_rejects_empty_and_duplicate_identities() {
    assert!(GroupRestorePlan::new(&[]).is_err());
    assert!(GroupRestorePlan::new(&[record("", None, None)]).is_err());
    assert!(GroupRestorePlan::new(&[
        record("same", None, Some("first")),
        record("same", None, Some("second")),
    ])
    .is_err());
}

#[test]
fn plan_rejects_parent_cycles_even_with_an_independent_root() {
    assert!(GroupRestorePlan::new(&[record("self", Some("self"), None)]).is_err());
    assert!(GroupRestorePlan::new(&[
        record("root", None, None),
        record("a", Some("b"), None),
        record("b", Some("a"), None),
    ])
    .is_err());
}
