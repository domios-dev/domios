use super::*;

#[test]
fn estimates_nested_payloads_without_encoding_string_bytes() {
    let plain = vec!["x".repeat(100_000)];
    let escaped = vec!["\n".repeat(100_000)];
    assert_eq!(estimate(&plain), estimate(&escaped));
    assert_eq!(
        estimate(&plain),
        100_000 + 2 * std::mem::size_of::<String>()
    );
    assert!(estimate(&serde_json::json!({"body": plain})) > 100_000);
}
