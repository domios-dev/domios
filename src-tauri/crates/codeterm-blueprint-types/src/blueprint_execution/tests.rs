use super::*;
use serde_json::json;

fn contract(schema: Value) -> ExecutionContract {
    ExecutionContract {
        input: Some(serde_json::from_value(schema).unwrap()),
        ..Default::default()
    }
}

#[test]
fn trigger_text_is_a_string_when_the_schema_accepts_a_string() {
    let first = contract(json!({"type": "string"}));
    for text in ["7", "{\"n\":7}", "true", "null", "plain"] {
        let envelope = ExecutionEnvelope::from_trigger_text(text, Some(&first));
        assert_eq!(envelope.media, "text");
        assert_eq!(envelope.value, json!(text));
    }
}

#[test]
fn trigger_text_without_a_contract_is_a_string() {
    let envelope = ExecutionEnvelope::from_trigger_text("7", None);
    assert_eq!(envelope.value, json!("7"));
    let envelope = ExecutionEnvelope::from_trigger_text("7", Some(&ExecutionContract::default()));
    assert_eq!(envelope.value, json!("7"));
}

#[test]
fn trigger_text_is_read_as_json_only_for_a_schema_that_rejects_strings() {
    let integer = contract(json!({"type": "integer"}));
    let envelope = ExecutionEnvelope::from_trigger_text("7", Some(&integer));
    assert_eq!(
        (envelope.media.as_str(), envelope.value),
        ("json", json!(7))
    );
    let object = contract(json!({"type": "object", "properties": {"n": {"type": "integer"}}}));
    let envelope = ExecutionEnvelope::from_trigger_text("{\"n\":7}", Some(&object));
    assert_eq!(envelope.value, json!({"n": 7}));
    let envelope = ExecutionEnvelope::from_trigger_text("{\"n\":\"x\"}", Some(&object));
    assert_eq!(envelope.value, json!("{\"n\":\"x\"}"));
    let envelope = ExecutionEnvelope::from_trigger_text("seven", Some(&integer));
    assert_eq!(envelope.value, json!("seven"));
}

#[test]
fn violation_names_the_schema_and_the_received_value() {
    let integer = contract(json!({"type": "integer"}));
    let text = ExecutionEnvelope::from_trigger_text("seven", Some(&integer));
    let detail = integer.violation(&text, false).unwrap().unwrap();
    assert!(detail.starts_with("input expected"), "{detail}");
    assert!(detail.contains("\"type\":\"integer\""), "{detail}");
    assert!(detail.contains("text \"seven\""), "{detail}");
    assert_eq!(
        integer.validate(&text, false),
        Err(JournalReason::ContractViolation)
    );
    let ok = ExecutionEnvelope::from_trigger_text("7", Some(&integer));
    assert_eq!(integer.violation(&ok, false), Ok(None));
}

#[test]
fn violation_detail_is_bounded() {
    let integer = contract(json!({"type": "integer"}));
    let long = ExecutionEnvelope::from_trigger_text(&"x".repeat(5000), Some(&integer));
    let detail = integer.violation(&long, false).unwrap().unwrap();
    assert!(detail.chars().count() < 200, "{detail}");
}
