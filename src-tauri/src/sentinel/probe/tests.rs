use super::Process;

#[cfg(unix)]
#[test]
fn sentinel_probe_owner_retirement_refuses_signal() {
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let mut process = Process {
        child,
        terminated: false,
    };
    let status = process.child.wait();
    process.terminated = true;
    assert!(status.unwrap().success());

    let mut calls = Vec::new();
    process.terminate_group_with(|pgid| calls.push(pgid));

    assert!(calls.is_empty(), "destructive group call after child reap");
}
