use super::*;

#[test]
fn malformed_error_arrays_keep_the_answer_parse_classification() {
    assert!(matches!(
        decode_result::<serde_json::Map<String, serde_json::Value>>(serde_json::json!(["timeout"])),
        Err(DecisionError::Parse(_))
    ));
    assert!(matches!(
        decode_result::<serde_json::Map<String, serde_json::Value>>(
            serde_json::json!({"error":"timeout"})
        ),
        Err(DecisionError::Deadline)
    ));
}
