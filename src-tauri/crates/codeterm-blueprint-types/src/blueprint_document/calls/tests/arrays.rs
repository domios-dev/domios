use super::*;

fn check_fan_in(checks: Value) -> (BTreeMap<String, VerifiedCall>, Vec<CallFinding>) {
    let tree = Tree::new();
    tree.raw(
        "checks.yaml",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../codeterm-mem/resources/blueprint-catalog/checks-fan-in.yaml"
        )),
    );
    tree.put("caller.yaml", &json!({
        "schema_version":2,"kind":"pipeline","name":"array-caller",
        "trigger":{"type":"schedule","every":{"lit":"20s"}},
        "imports":{"checks":"checks.yaml"},
        "steps":[{"id":"gate","call":{"ref":"checks","with":{"args":{"lit":{"checks":checks}}}}}],
        "result":{"to":"notify"}
    }));
    check_facts(&tree.resolve("caller.yaml").unwrap())
}

#[test]
fn literal_array_catalog_checks_fan_in_accepts_differing_objects() {
    let (verified, findings) = check_fan_in(json!([
        {"name":"build","ok":true,"detail":"ok"},
        {"name":"unit tests","ok":false,"detail":"one failing test"}
    ]));
    assert!(findings.is_empty(), "{findings:?}");
    assert!(verified.contains_key("gate"));
}

#[test]
fn literal_array_catalog_checks_fan_in_accepts_empty() {
    let (verified, findings) = check_fan_in(json!([]));
    assert!(findings.is_empty(), "{findings:?}");
    assert!(verified.contains_key("gate"));
}

#[test]
fn literal_array_catalog_checks_fan_in_rejects_invalid_later_item() {
    let (verified, findings) = check_fan_in(json!([
        {"name":"build","ok":true,"detail":"ok"},
        {"name":"tests","ok":"false","detail":"bad type"}
    ]));
    assert!(!verified.contains_key("gate"));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "C4");
    assert_eq!(findings[0].field, "steps[0].call.with.args");
}
