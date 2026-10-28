use super::*;
use serde_json::json;

fn controls(names: &[&str]) -> Vec<AxNode> {
    let children: Vec<String> = (0..names.len()).map(|i| format!("n{i}")).collect();
    let mut rows =
        vec![json!({"nodeId": "root", "role": {"value": "RootWebArea"}, "childIds": children})];
    rows.extend(names.iter().enumerate().map(|(i, name)| {
        json!({"nodeId": format!("n{i}"), "parentId": "root", "role": {"value": "button"},
            "name": {"value": name}, "backendDOMNodeId": i + 1})
    }));
    serde_json::from_value(Value::Array(rows)).unwrap()
}

#[test]
fn ambiguous_exact_names_require_an_explicit_ref() {
    let snapshot = build_snapshot(&controls(&["Delete", "Delete", "Delete draft"]), 20);
    assert_eq!(
        resolve_target(Some(&snapshot), &["button", "delete"]),
        Err(ResolveError::Ambiguous {
            role: "button".into(),
            name: "delete".into(),
            refs: vec!["@e1".into(), "@e2".into()],
        })
    );
    assert_eq!(
        resolve_target(Some(&snapshot), &["@e2"])
            .unwrap()
            .backend_node_id,
        2
    );
}

#[test]
fn unique_exact_name_wins_over_multiple_partial_names() {
    let snapshot = build_snapshot(&controls(&["Save draft", "Save", "Save copy"]), 20);
    assert_eq!(
        resolve_target(Some(&snapshot), &["button", "save"])
            .unwrap()
            .backend_node_id,
        2
    );
    assert!(matches!(
        resolve_target(Some(&snapshot), &["button", "sav"]),
        Err(ResolveError::Ambiguous { .. })
    ));
}

#[test]
fn old_capture_refs_cannot_alias_the_next_snapshot() {
    let old = build_snapshot_scoped(&controls(&["Preview"]), 20, "first");
    let new = build_snapshot_scoped(&controls(&["Submit"]), 20, "second");
    assert_eq!(
        resolve_target(Some(&new), &["@first-e1"]),
        Err(ResolveError::StaleRef("@first-e1".into()))
    );
    assert_eq!(
        resolve_target(Some(&old), &["@first-e1"])
            .unwrap()
            .backend_node_id,
        1
    );
    assert_eq!(
        resolve_target(Some(&new), &["@second-e1"])
            .unwrap()
            .backend_node_id,
        1
    );
    assert_eq!(new.index[0].reference, "second-e1");
}

#[test]
fn snapshot_exposes_control_states_without_treating_false_as_disabled() {
    let nodes: Vec<AxNode> = serde_json::from_value(json!([{
        "nodeId": "control", "role": {"value": "checkbox"}, "backendDOMNodeId": 1,
        "properties": [
            {"name": "checked", "value": {"value": "mixed"}},
            {"name": "expanded", "value": {"value": false}},
            {"name": "selected", "value": {"value": true}},
            {"name": "disabled", "value": {"value": "false"}},
            {"name": "required", "value": {"value": true}},
            {"name": "invalid", "value": {"value": "grammar"}}
        ]
    }]))
    .unwrap();
    let snapshot = build_snapshot(&nodes, 20);
    for state in [
        "[checked=mixed]",
        "[expanded=false]",
        "[selected=true]",
        "[required]",
        "[invalid]",
    ] {
        assert!(snapshot.text.contains(state), "{state}");
    }
    assert!(!snapshot.text.contains("[disabled]"));
}

#[test]
fn exact_line_limit_does_not_report_nonexistent_truncation() {
    let snapshot = build_snapshot(&controls(&["Only"]), 1);
    assert_eq!(snapshot.index.len(), 1);
    assert!(!snapshot.text.contains("truncated"));
    let truncated = build_snapshot(&controls(&["First", "Second"]), 1);
    assert_eq!(truncated.index.len(), 1);
    assert!(truncated.text.contains("truncated"));
}

#[test]
fn snapshot_caps_output_even_when_the_caller_requests_an_unbounded_limit() {
    let names = vec!["Control"; MAX_LINES + 1];
    let snapshot = build_snapshot(&controls(&names), usize::MAX);
    assert_eq!(snapshot.index.len(), MAX_LINES);
    assert!(snapshot
        .text
        .contains(&format!("truncated at {MAX_LINES} lines")));
}

#[test]
fn bounded_display_preserves_full_names_for_legacy_role_locators() {
    let name = "x".repeat(150);
    let snapshot = build_snapshot(&controls(&[&name]), 20);
    assert_eq!(snapshot.items[0].name.len(), NAME_LIMIT);
    assert_eq!(
        resolve_target(Some(&snapshot), &["button", &name])
            .unwrap()
            .backend_node_id,
        1
    );
}
