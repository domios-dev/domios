use super::decode_response;
use reqwest::StatusCode;

#[test]
fn snapshot_refusal_checks_status_before_json_and_preserves_text() {
    let refusal = "tab=fixture: transcript capture unavailable: opaque reason\n";
    let error = decode_response(StatusCode::BAD_REQUEST, refusal.as_bytes()).unwrap_err();
    assert_eq!(
        error,
        format!("Snapshot exchange refused: 400 Bad Request {refusal}")
    );
    assert!(!error.contains("Invalid snapshot response"));
}

#[test]
fn snapshot_refusal_bounds_text_at_a_utf8_boundary() {
    let refusal = format!("{}🙂private-tail", "x".repeat(4095));
    let error = decode_response(StatusCode::FORBIDDEN, refusal.as_bytes()).unwrap_err();
    assert_eq!(
        error,
        format!(
            "Snapshot exchange refused: 403 Forbidden {} [truncated]",
            "x".repeat(4095)
        )
    );
}

#[test]
fn snapshot_success_parses_json_and_rejects_malformed_documents() {
    assert_eq!(
        decode_response(StatusCode::OK, b"{\"sanitized\":true}").unwrap()["sanitized"],
        true
    );
    assert!(decode_response(StatusCode::OK, b"not json")
        .unwrap_err()
        .starts_with("Invalid snapshot response:"));
}
