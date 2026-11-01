use super::*;

const FIXTURE: &str = include_str!("../fixtures/ax_tree_login.json");
const EXPECTED: &str = "- navigation \"Primary\"
  - link \"Home\" [@e1]
  - link \"Pricing\" [@e2]
- main
  - heading \"Sign in to Acme\"
  - form
    - textbox \"Email\": andrey@example.com [required] [@e3]
    - textbox \"Password\" [@e4]
    - checkbox \"Remember me\" [checked=false] [@e5]
    - slider \"Session length\": 30 [@e6]
    - button \"Sign in\" [@e7]
    - button \"Continue with SSO\" [disabled] [@e8]
    - link \"Forgot password?\" [@e9]
  - list
    - listitem
      - link \"Terms\"
    - listitem \"Status: all systems normal\"
      - img
      - tab \"Sign in\" [@e10]";

fn fixture_nodes() -> Vec<AxNode> {
    let value: Value = serde_json::from_str(FIXTURE).unwrap();
    parse_full_ax_tree(&value).unwrap()
}

fn fixture_snapshot() -> Snapshot {
    build_snapshot(&fixture_nodes(), DEFAULT_MAX_LINES)
}

#[test]
fn browser_snapshot_matches_poc_output_for_recorded_tree() {
    assert_eq!(fixture_snapshot().text, EXPECTED);
}

#[test]
fn browser_snapshot_maps_refs_to_backend_node_ids() {
    let snapshot = fixture_snapshot();
    let expected = [
        ("e1", 9),
        ("e2", 11),
        ("e3", 24),
        ("e4", 25),
        ("e5", 26),
        ("e6", 27),
        ("e7", 28),
        ("e8", 29),
        ("e9", 31),
        ("e10", 45),
    ];
    assert_eq!(snapshot.refs.len(), expected.len());
    for (reference, id) in expected {
        assert_eq!(snapshot.refs.get(reference), Some(&id), "{reference}");
    }
    assert_eq!(snapshot.index[2].role, "textbox");
    assert_eq!(snapshot.index[2].name, "Email");
}

#[test]
fn browser_snapshot_truncates_at_line_limit() {
    let snapshot = build_snapshot(&fixture_nodes(), 4);
    assert_eq!(
            snapshot.text,
            "- navigation \"Primary\"\n  - link \"Home\" [@e1]\n  - link \"Pricing\" [@e2]\n- main\n… (truncated at 4 lines; pass a larger limit)"
        );
    assert_eq!(snapshot.refs.len(), 2);
}

#[test]
fn browser_snapshot_survives_cycles_and_empty_trees() {
    assert_eq!(build_snapshot(&[], DEFAULT_MAX_LINES).text, "");
    let cyclic: Vec<AxNode> = serde_json::from_value(serde_json::json!([
            {"nodeId": "a", "role": {"value": "button"}, "name": {"value": "Go"}, "childIds": ["b"], "backendDOMNodeId": 3},
            {"nodeId": "b", "parentId": "a", "role": {"value": "link"}, "name": {"value": "Back"}, "childIds": ["a"], "backendDOMNodeId": 4}
        ]))
        .unwrap();
    let snapshot = build_snapshot(&cyclic, DEFAULT_MAX_LINES);
    assert_eq!(
        snapshot.text,
        "- button \"Go\" [@e1]\n  - link \"Back\" [@e2]"
    );
}

#[test]
fn browser_snapshot_truncates_long_names_by_chars() {
    let long = "→".repeat(150);
    let nodes: Vec<AxNode> = serde_json::from_value(serde_json::json!([
        {"nodeId": "a", "role": {"value": "heading"}, "name": {"value": long}}
    ]))
    .unwrap();
    let text = build_snapshot(&nodes, DEFAULT_MAX_LINES).text;
    assert_eq!(text, format!("- heading \"{}\"", "→".repeat(NAME_LIMIT)));
}

#[test]
fn browser_resolve_by_ref_and_stale_ref() {
    let snapshot = fixture_snapshot();
    assert_eq!(
        resolve_target(Some(&snapshot), &["@e7"]),
        Ok(ResolvedTarget {
            backend_node_id: 28,
            label: "@e7".into()
        })
    );
    assert_eq!(
        resolve_target(Some(&snapshot), &["@e99"]),
        Err(ResolveError::StaleRef("@e99".into()))
    );
}

#[test]
fn browser_resolve_by_role_and_name_prefers_exact_then_substring() {
    let snapshot = fixture_snapshot();
    let exact = resolve_target(Some(&snapshot), &["button", "sign in"]).unwrap();
    assert_eq!(exact.backend_node_id, 28);
    assert_eq!(exact.label, "button \"Sign in\"");
    let partial = resolve_target(Some(&snapshot), &["button", "sso"]).unwrap();
    assert_eq!(partial.backend_node_id, 29);
    let tab = resolve_target(Some(&snapshot), &["tab", "Sign in"]).unwrap();
    assert_eq!(tab.backend_node_id, 45);
}

#[test]
fn browser_resolve_reports_typed_errors() {
    let snapshot = fixture_snapshot();
    assert_eq!(
        resolve_target(None, &["@e1"]),
        Err(ResolveError::NoSnapshot)
    );
    assert_eq!(
        resolve_target(Some(&snapshot), &[]),
        Err(ResolveError::MissingTarget)
    );
    assert_eq!(
        resolve_target(Some(&snapshot), &["button", "Delete account"]),
        Err(ResolveError::NotFound {
            role: "button".into(),
            name: "Delete account".into()
        })
    );
}
