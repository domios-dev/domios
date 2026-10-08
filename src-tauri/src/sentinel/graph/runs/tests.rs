use super::*;

fn node(id: &str, run: &str, transition: &str, parent: Value) -> Value {
    json!({"identity":{"id":id},"history":[{
        "id":transition,"handlers":[],"causal_run":{
            "id":run,"root_node":"root","root_transition":run,"started_at":10,"parent":parent
        }
    }]})
}

#[test]
fn sentinel_graph_runs_group_by_persisted_identity_with_equal_times() {
    let mut root = node("root", "one", "one", Value::Null);
    root["history"]
        .as_array_mut()
        .unwrap()
        .push(node("root", "two", "two", Value::Null)["history"][0].clone());
    let child = node(
        "child",
        "one",
        "child-result",
        json!({"node":"root","transition":"one","handler":"activate"}),
    );
    let groups = project(&[root.clone(), child.clone()], &[], 10, &BTreeSet::new());
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["transitions"].as_array().unwrap().len(), 2);
    assert_eq!(groups[1]["transitions"].as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["incomplete"], false);
    root["history"][0]["pending_handlers"] = json!(true);
    assert_eq!(
        project(&[root.clone()], &[], 10, &BTreeSet::new())[0]["state"],
        "unconfirmed"
    );
    assert_eq!(
        project(&[root], &[], 10, &BTreeSet::from(["root".into()]))[0]["state"],
        "running"
    );
    let truncated = project(&[child], &[], 10, &BTreeSet::new());
    assert_eq!(truncated[0]["incomplete"], true);
}
