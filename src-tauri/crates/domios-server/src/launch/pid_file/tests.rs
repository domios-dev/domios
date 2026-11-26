use super::*;

#[test]
fn publishes_only_the_ready_pid_and_refuses_live_holder() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("server.pid");
    let file = PidFile::prepare(&destination).unwrap();
    assert!(!destination.exists());
    let pid = std::process::id();
    file.publish(pid).unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        format!("{pid}\n")
    );
    assert!(PidFile::prepare(&destination).is_err());
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        format!("{pid}\n")
    );
}

#[test]
fn failed_start_leaves_no_pid_file() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("server.pid");
    drop(PidFile::prepare(&destination).unwrap());
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn replaces_exited_holder_only_after_ready_and_serializes_recovery() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("server.pid");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let old = format!("{}\n", child.id());
    assert!(child.wait().unwrap().success());
    std::fs::write(&destination, &old).unwrap();
    let recovery = PidFile::prepare(&destination).unwrap();
    assert!(PidFile::prepare(&destination).is_err());
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), old);
    let pid = std::process::id();
    recovery.publish(pid).unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        format!("{pid}\n")
    );
}
