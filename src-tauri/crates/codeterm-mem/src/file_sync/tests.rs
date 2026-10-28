use super::*;

#[test]
fn file_sync_bounded_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tracked.txt");
    std::fs::write(&path, vec![b'x'; TRACKED_SOURCE_READ_LIMIT as usize + 1]).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    let error = read_tracked_source(&path, size).unwrap_err();
    assert!(error.contains("4194304"));
}
