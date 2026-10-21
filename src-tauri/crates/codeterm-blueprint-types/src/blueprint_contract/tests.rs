use super::*;

#[test]
fn typed_apply_error_round_trip_preserves_colons_and_document_path() {
    let error = ApplyError::CreationOnly {
        field: "watch.timeout".into(),
        message: "creation: only; target: service".into(),
    };
    let wire = serde_json::to_value(&error).unwrap();
    let decoded: ApplyError = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(decoded, ApplyError::CreationOnly { field, message } if field == "watch.timeout" && message == "creation: only; target: service")
    );
}
