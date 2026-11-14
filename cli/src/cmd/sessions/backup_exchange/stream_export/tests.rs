use super::*;

#[test]
fn export_stream_exceeds_rpc_collector_without_materializing_history() {
    let prefix = br#"{"format":"upgrade-restore-v2","sanitized":true,"history":""#;
    let payload = std::io::repeat(b'x').take(65 * 1024 * 1024);
    let response = prefix.as_slice().chain(payload).chain(&b"\"}"[..]);
    let mut file = tempfile::tempfile().unwrap();
    receive(response, &mut file).unwrap();
    assert_eq!(
        file.metadata().unwrap().len(),
        prefix.len() as u64 + 65 * 1024 * 1024 + 2
    );
}

#[test]
fn export_stream_validates_entire_document_and_sanitization_before_publication() {
    for invalid in [
        r#"{"format":"upgrade-restore-v2","sanitized":false}"#,
        r#"{"format":"upgrade-restore-v1","sanitized":true}"#,
        r#"{"format":"upgrade-restore-v2","sanitized":true,"history":[}"#,
        r#"{"format":"upgrade-restore-v2","sanitized":true} trailing"#,
    ] {
        let mut file = tempfile::tempfile().unwrap();
        assert!(receive(invalid.as_bytes(), &mut file).is_err());
    }
}
