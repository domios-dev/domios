use super::*;

fn parse(raw: &str) -> Result<Value, String> {
    let value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    validate_handler_file(value, &["passed".into(), "waiting".into()])
}

#[test]
fn missing_ids_are_generated_from_type_and_bucket() {
    let handlers = parse(r#"{"passed":[{"type":"notify","parameters":{}}]}"#).unwrap();
    assert_eq!(handlers["passed"][0]["id"], "notify-on-passed");
}

#[test]
fn precise_pointer_errors_describe_expected_allowed_values_and_correction() {
    let error = parse(r#"{"passed":[{"type":"notify","parameters":{}},{"type":"send","parameters":{"tab":"tab_1"}}]}"#).unwrap_err();
    assert!(error.contains("/passed/1/parameters/tab"));
    assert!(error.contains("expected a supported field"));
    assert!(error.contains("allowed values: target"));
    assert!(error.contains("\"parameters\":{\"target\":\"target_id\"}"));
}

#[test]
fn refuses_generic_ids_and_unknown_keys_by_name() {
    for id in ["default_1", "handler_2"] {
        let generic = parse(&format!(
            r#"{{"passed":[{{"id":"{id}","type":"notify","parameters":{{}}}}]}}"#
        ))
        .unwrap_err();
        assert!(generic.contains("omit id to generate a type-and-bucket name"));
    }
    let unknown =
        parse(r#"{"passed":[{"id":"review","type":"notify","parameters":{},"unexpected":true}]}"#)
            .unwrap_err();
    assert!(unknown.contains("/passed/0/unexpected"));
    assert!(unknown.contains("unexpected"));
}
