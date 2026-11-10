use super::*;
use codeterm_blueprint_types::blueprint_run::{Context, Parent};
use codeterm_sentinel::graph_contracts::{DecisionProjection, Node, Transition};
use serde_json::json;

fn node(id: &str, run: &str, transition: &str, parent: Option<Parent>) -> Node {
    Node {
        identity: super::super::Identity {
            id: id.into(),
            ..Default::default()
        },
        history: Some(vec![Transition {
            id: transition.into(),
            bucket: "done".into(),
            accepted_at: 10,
            routes_current: true,
            pending_handlers: false,
            handlers: Vec::new(),
            decision: DecisionProjection::default(),
            causal_run: Some(Context {
                id: run.into(),
                root_node: "root".into(),
                root_transition: run.into(),
                started_at: 10,
                parent,
            }),
        }]),
        ..Default::default()
    }
}
#[test]
fn sentinel_graph_runs_group_by_persisted_identity_with_equal_times() {
    let mut root = node("root", "one", "one", None);
    root.history
        .as_mut()
        .unwrap()
        .extend(node("root", "two", "two", None).history.unwrap());
    let child = node(
        "child",
        "one",
        "child-result",
        Some(Parent {
            node: "root".into(),
            transition: "one".into(),
            handler: "activate".into(),
        }),
    );
    let groups = project(&[root.clone(), child.clone()], &[], 10, &BTreeSet::new());
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].transitions.len(), 2);
    assert_eq!(groups[1].transitions.len(), 1);
    assert!(!groups[0].incomplete);
    root.history.as_mut().unwrap()[0].pending_handlers = true;
    assert_eq!(
        json!(project(&[root.clone()], &[], 10, &BTreeSet::new()))[0]["state"],
        "unconfirmed"
    );
    assert_eq!(
        json!(project(&[root], &[], 10, &BTreeSet::from(["root".into()])))[0]["state"],
        "running"
    );
    assert!(project(&[child], &[], 10, &BTreeSet::new())[0].incomplete);
}
