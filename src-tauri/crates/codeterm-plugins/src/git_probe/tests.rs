use super::*;

#[test]
fn expired_probe_budget_never_starts_a_process() {
    assert!(capture_observed(
        Path::new("unavailable-git-probe"),
        Instant::now(),
        epoch(),
        |_| panic!("expired probe spawned"),
    )
    .is_none());
}

#[cfg(unix)]
fn script(root: &Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn parked(root: &Path) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let fifo = root.join("gate");
    let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    script(
        root,
        "parked",
        &format!(
            "printf 'git version 2.21.0\\n'; exec /bin/cat '{}'",
            fifo.display()
        ),
    )
}

#[cfg(unix)]
fn assert_reaped(pid: u32) {
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[cfg(unix)]
#[test]
fn probe_deadline_reaps_parked_candidate_and_allows_later_usable_git() {
    let _guard = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut pid = None;
    let started = Instant::now();
    assert!(capture_observed(
        &parked(dir.path()),
        started + Duration::from_millis(100),
        epoch(),
        |id| pid = Some(id)
    )
    .is_none());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_reaped(pid.unwrap());
    let good = script(dir.path(), "good", "printf 'git version 2.50.0\\n'");
    assert_eq!(
        capture(&good, Instant::now() + Duration::from_secs(2), epoch()).unwrap(),
        b"git version 2.50.0\n"
    );
    assert!(PROBES.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn shutdown_cancels_and_reaps_probe_and_rejects_old_discovery_epoch() {
    let _guard = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let path = parked(dir.path());
    let admitted = epoch();
    let (started, ready) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        capture_observed(
            &path,
            Instant::now() + Duration::from_secs(5),
            admitted,
            |pid| started.send(pid).unwrap(),
        )
    });
    let pid = ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(stop(), 1);
    assert!(worker.join().unwrap().is_none());
    assert_reaped(pid);
    assert!(PROBES.lock().unwrap().is_empty());
    let good = script(dir.path(), "good", "printf 'git version 2.50.0\\n'");
    assert!(capture(&good, Instant::now() + Duration::from_secs(2), admitted).is_none());
}

#[cfg(unix)]
#[test]
fn probe_output_is_bounded_and_expired_budget_never_spawns() {
    let _guard = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let path = script(dir.path(), "large", "exec /usr/bin/yes version");
    assert!(capture(&path, Instant::now() + Duration::from_secs(2), epoch()).is_none());
    assert!(capture_observed(&path, Instant::now(), epoch(), |_| panic!(
        "expired probe spawned"
    ))
    .is_none());
    assert!(PROBES.lock().unwrap().is_empty());
}
