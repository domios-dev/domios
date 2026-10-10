use super::*;
use std::time::Duration;

#[test]
fn source_watcher_registers_before_parent_and_file_materialize() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("project").join("session.jsonl");
    let (tx, rx) = mpsc::sync_channel(1);
    let watcher = register_source(&path, tx).unwrap();
    assert!(!path.exists());
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{}\n").unwrap();
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"{}\n");
    drop(watcher);
}

#[test]
fn source_watcher_preserves_absence_boundary_until_materialization() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    let source = crate::service::tab_chat::registry::JsonlSource::default();
    assert_eq!(
        capture_window(&source, "session", &path),
        Some(EvidenceWindow::Jsonl {
            transcript_offset: 0
        })
    );
    std::fs::write(&path, b"{}\n").unwrap();
    assert_eq!(
        capture_window(&source, "session", &path),
        Some(EvidenceWindow::Jsonl {
            transcript_offset: 3
        })
    );
}
