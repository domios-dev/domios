use super::*;

#[test]
fn evidence_retains_the_last_stage_and_target_without_claiming_installation() {
    let dir = tempfile::tempdir().unwrap();
    let evidence = Evidence {
        target_version: "2.0.0".into(),
        stage: String::new(),
        message: None,
        owner_pid: 10,
        updated_at_ms: 0,
        data: dir.path().into(),
    };
    evidence.record("server_stopped", None).unwrap();
    evidence
        .record("installer_call_failed", Some("access refused"))
        .unwrap();
    let saved: Evidence =
        serde_json::from_slice(&std::fs::read(path(dir.path())).unwrap()).unwrap();
    assert_eq!(saved.stage, "installer_call_failed");
    assert_eq!(saved.message.as_deref(), Some("access refused"));
    let diagnostic = diagnostic(dir.path(), Some("2.0.0"));
    assert!(diagnostic.contains("installer_call_failed"));
    assert!(diagnostic.contains("native-update-installer.json"));
    assert!(super::diagnostic(dir.path(), Some("3.0.0")).contains("No matching"));
}
