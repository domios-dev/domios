use super::*;

#[test]
fn blueprint_document_identity_ignores_nested_object_insertion_order() {
    let insertion: Value =
        serde_json::from_str(r#"{"z":[{"z":2,"a":1}],"a":{"z":4,"a":3}}"#).unwrap();
    let mut sorted = insertion.clone();
    sorted.sort_all_objects();

    let mut document: Document = serde_json::from_value(json!({
        "schema_version": 2,
        "kind": "pipeline",
        "name": "object-order",
        "steps": [{"id": "emit", "run": {"lit": "echo ready"}}]
    }))
    .unwrap();
    document.steps[0]
        .params
        .insert("payload".into(), ValueForm::literal(insertion));
    let mut reordered = document.clone();
    reordered.steps[0]
        .params
        .insert("payload".into(), ValueForm::literal(sorted));
    assert_eq!(json(&document).unwrap(), json(&reordered).unwrap());
    assert_eq!(identity(&document).unwrap(), identity(&reordered).unwrap());
    let projected: Value = serde_json::from_str(&json(&document).unwrap()).unwrap();
    assert_eq!(
        projected["document"]["steps"][0]["with"]["payload"]["lit"].to_string(),
        r#"{"a":{"a":3,"z":4},"z":[{"a":1,"z":2}]}"#
    );

    let mut changed = document.clone();
    changed.steps[0].params.insert(
        "payload".into(),
        ValueForm::literal(json!({"z": [{"z": 1, "a": 2}], "a": {"z": 4, "a": 3}})),
    );
    assert_ne!(identity(&document).unwrap(), identity(&changed).unwrap());
}

#[test]
fn blueprint_document_identity_golden() {
    let path = crate::test_support::fixture_manifest_dir()
        .join("../../../tests/fixtures/blueprint-identities.golden.json");
    let mut fixtures: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let update = std::env::var_os("UPDATE_BLUEPRINT_IDENTITY_GOLDEN").is_some();
    for fixture in fixtures.as_array_mut().unwrap() {
        if update && fixture["name"] == "call" {
            let frozen = fixture["document"]["dependencies"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()
                .clone();
            let mut frozen = frozen;
            let child: Document = serde_json::from_value(frozen["document"].clone()).unwrap();
            let hash = identity(&child).unwrap();
            frozen["hash"] = json!(hash);
            fixture["document"]["dependencies"] =
                json!({format!("mem:identity-helper@{hash}"):frozen});
            fixture["document"]["steps"][0]["call"]["pin"]["hash"] = json!(hash);
        }
        let doc: Document = serde_json::from_value(fixture["document"].clone()).unwrap();
        assert!(
            doc.template_errors().is_empty(),
            "{:?}",
            doc.template_errors()
        );
        let hash = identity(&doc).unwrap();
        if update {
            fixture["hash"] = json!(hash);
        }
        assert_eq!(
            hash,
            fixture["hash"].as_str().unwrap(),
            "{}",
            fixture["name"]
        );
        let roundtrip: Document =
            serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
        assert_eq!(identity(&roundtrip).unwrap(), hash);
        for callee in doc.dependencies.values() {
            callee.verify().unwrap();
        }
    }
    if update {
        std::fs::write(
            path,
            format!("{}\n", serde_json::to_string_pretty(&fixtures).unwrap()),
        )
        .unwrap();
    }
}

#[test]
fn blueprint_document_default_fields_do_not_enter_identity() {
    let minimal = json!({"schema_version":2,"kind":"pipeline","name":"defaults","trigger":{"type":"event","kind":{"lit":"tab_output"}},"steps":[{"id":"code","code":{"lang":"python","src":"def main(value): return value"}}]});
    let mut explicit = minimal.clone();
    explicit["result"] = json!({});
    explicit["trigger"]["params"] = json!({});
    for name in ["model_classification", "session", "paused"] {
        explicit["steps"][0][name] = json!(false);
    }
    explicit["steps"][0]["code"]["limits"] = json!({});
    let minimal: Document = serde_json::from_value(minimal).unwrap();
    let explicit: Document = serde_json::from_value(explicit).unwrap();
    assert_eq!(identity(&minimal), identity(&explicit));
    let value = serde_json::to_value(explicit).unwrap();
    assert!(value.get("result").is_none());
    assert!(value["trigger"].get("params").is_none());
    for name in ["model_classification", "session", "paused"] {
        assert!(value["steps"][0].get(name).is_none());
    }
    assert!(value["steps"][0]["code"].get("limits").is_none());
}
