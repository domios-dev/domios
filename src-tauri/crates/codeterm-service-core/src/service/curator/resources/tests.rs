use super::*;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for name in ["lock", "queue", "jobs"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    root
}

#[test]
fn curator_resources_reads_active_gateway_and_bounded_queue() {
    let root = fixture();
    std::fs::remove_dir(root.path().join("lock")).unwrap();
    assert!(from_directory(root.path())
        .unwrap()
        .contains("gateway idle, queue 0"));
    std::fs::create_dir(root.path().join("lock")).unwrap();
    std::fs::write(root.path().join("lock/job_id"), "job-1\n").unwrap();
    std::fs::write(
        root.path().join("jobs/job-1.json"),
        br#"{"state":"running","owner":"tab-fixture","command":"test-one crate selector"}"#,
    )
    .unwrap();
    for index in 0..QUEUE_LIMIT + 1 {
        std::fs::create_dir(root.path().join("queue").join(index.to_string())).unwrap();
    }
    let line = from_directory(root.path()).unwrap();
    assert!(line.contains("gateway tab-fixture/test-one crate selector, queue 128+"));
    assert!(line.contains("one shared cargo slot"));
}

#[test]
fn curator_resources_omits_unreadable_oversized_or_unsafe_state() {
    let root = fixture();
    std::fs::write(root.path().join("lock/job_id"), "../other").unwrap();
    assert_eq!(from_directory(root.path()), None);
    std::fs::write(root.path().join("lock/job_id"), "job-1").unwrap();
    std::fs::write(
        root.path().join("jobs/job-1.json"),
        vec![b'x'; RECORD_LIMIT + 1],
    )
    .unwrap();
    assert_eq!(from_directory(root.path()), None);
    std::fs::write(root.path().join("jobs/job-1.json"), b"{").unwrap();
    assert_eq!(from_directory(root.path()), None);
    assert!(clean(&"x\n|".repeat(100), 64).len() <= 64);
    assert!(!clean("x\n|", 64).contains(['\n', '|']));
}
