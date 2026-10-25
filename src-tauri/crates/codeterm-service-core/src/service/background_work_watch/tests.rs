use super::*;
use std::io::Write;
fn decode(value: &serde_json::Value) -> Option<RichMessage> {
    Some(RichMessage {
        provider_observations: Some(codeterm_provider_signals::SessionObservations {
            permission_mode: Some(codeterm_provider_signals::PermissionMode::Plan),
            ..Default::default()
        }),
        provider_session_id: value["sessionId"].as_str().map(str::to_string),
        ..Default::default()
    })
}
#[test]
fn restart_guard_event_reader_holds_partial_records_and_advances_once() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(
        file,
        "{{\"sessionId\":\"own\"}}\n{{\"sessionId\":\"next\"}}"
    )
    .unwrap();
    file.flush().unwrap();
    let (rows, offset, incomplete, partial) = read_file(file.path(), 0, decode).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!incomplete);
    assert!(partial);
    assert_eq!(rows[0].provider_session_id.as_deref(), Some("own"));
    let (rows, held, _, _) = read_file(file.path(), offset, decode).unwrap();
    assert!(rows.is_empty());
    assert_eq!(held, offset);
    writeln!(file).unwrap();
    file.flush().unwrap();
    let (rows, end, _, partial) = read_file(file.path(), offset, decode).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(end > offset);
    assert!(!partial);
    assert!(read_file(file.path(), end, decode).unwrap().0.is_empty());
}
#[test]
fn restart_guard_event_reader_truncation_fails_closed() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{{\"sessionId\":\"own\"}}").unwrap();
    file.flush().unwrap();
    assert!(read_file(file.path(), 1000, decode).unwrap().2);
}
