use super::*;

#[test]
fn publishes_only_the_ready_pid_and_refuses_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("server.pid");
    let file = PidFile::prepare(&destination).unwrap();
    assert!(!destination.exists());
    file.publish(1234).unwrap();
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), "1234\n");
    assert!(PidFile::prepare(&destination).is_err());
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), "1234\n");
}

#[test]
fn failed_start_leaves_no_pid_file() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("server.pid");
    drop(PidFile::prepare(&destination).unwrap());
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
