use super::*;
use std::io::{Read, Write};
use std::time::Duration;

#[test]
#[ignore = "subprocess fixture"]
fn probe_child_fixture() {
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };
    assert!(!event.is_null());
    let event = unsafe { OwnedHandle::from_raw_handle(event) };
    println!("\nprobe-child-ready");
    std::io::stdout().flush().unwrap();
    unsafe { WaitForSingleObject(event.as_raw_handle(), INFINITE) };
}

#[test]
#[ignore = "subprocess fixture"]
fn probe_child_version_fixture() {
    println!("\ncodex-cli 0.172.0");
    std::io::stdout().flush().unwrap();
}

fn read_until_line(stdout: OwnedHandle, needle: &str, timeout: Duration) -> io::Result<bool> {
    let mut reader = OverlappedReader::new(stdout)?;
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    loop {
        if reader.harvest_if_ready(&mut output)?.unwrap_or(false) {
            return Ok(String::from_utf8_lossy(&output)
                .lines()
                .any(|line| line == needle));
        }
        let done = reader.start_or_drain(&mut output, 64 * 1024)?;
        if String::from_utf8_lossy(&output)
            .lines()
            .any(|line| line == needle)
        {
            reader.close();
            return Ok(true);
        }
        if done {
            reader.close();
            return Ok(false);
        }
        let now = Instant::now();
        if now >= deadline {
            reader.close();
            return Ok(false);
        }
        let wait_ms = deadline
            .saturating_duration_since(now)
            .as_millis()
            .min(u128::from(u32::MAX - 1)) as u32;
        match unsafe { WaitForSingleObject(reader.event_handle(), wait_ms) } {
            WAIT_OBJECT_0 => {
                if reader.harvest_completed(&mut output)? {
                    reader.close();
                    return Ok(String::from_utf8_lossy(&output)
                        .lines()
                        .any(|line| line == needle));
                }
            }
            WAIT_TIMEOUT => {
                reader.close();
                return Ok(false);
            }
            _ => {
                reader.close();
                return Err(io::Error::last_os_error());
            }
        }
    }
}

#[test]
fn provider_version_owner_drop_kills_child_and_excludes_unrelated_handles() {
    let (mut unrelated_reader, unrelated_writer) = io::pipe().unwrap();
    assert_ne!(
        unsafe {
            SetHandleInformation(
                unrelated_writer.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        },
        0
    );
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "windows_job::probe_child::tests::probe_child_fixture",
        "--ignored",
        "--nocapture",
    ]);
    let mut owner = ProbeChild::spawn(&mut command).unwrap();
    let process = owner.process.try_clone().unwrap();
    let stdout = owner.stdout.take().unwrap();
    let ready = read_until_line(stdout, "probe-child-ready", Duration::from_secs(5));
    if !matches!(ready, Ok(true)) {
        drop(owner);
        panic!("probe child did not signal readiness: {ready:?}");
    }
    drop(unrelated_writer);
    let (eof_tx, eof_rx) = std::sync::mpsc::sync_channel(1);
    let unrelated = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = eof_tx.send(unrelated_reader.read_to_end(&mut bytes));
    });
    let unrelated_eof = eof_rx.recv_timeout(Duration::from_secs(5));
    drop(owner);
    let exited = unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
    unrelated.join().unwrap();
    assert_eq!(
        exited, WAIT_OBJECT_0,
        "owner drop must leave no running child"
    );
    assert_eq!(
        unrelated_eof.unwrap().unwrap(),
        0,
        "unrelated pipe must close while child runs"
    );
}

#[test]
fn probe_child_collect_output_completes_when_foreign_handle_keeps_stdout_open() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "windows_job::probe_child::tests::probe_child_version_fixture",
        "--ignored",
        "--nocapture",
    ]);
    let (mut child, leaked_writer) = ProbeChild::spawn_inner(&mut command, true).unwrap();
    let _leaked_writer = leaked_writer.expect("retained stdout writer handle");
    let started = Instant::now();
    let result = child
        .collect_output(started + Duration::from_secs(5), 64 * 1024)
        .unwrap();
    let (status, bytes) = result.expect("probe must not time out when job is empty");
    assert!(status.success(), "fixture exited cleanly: {status:?}");
    assert!(
        String::from_utf8_lossy(&bytes)
            .lines()
            .any(|line| line.trim() == "codex-cli 0.172.0"),
        "expected version output, got {:?}",
        String::from_utf8_lossy(&bytes)
    );
}

#[test]
fn probe_child_collect_output_times_out_within_bound_when_child_hangs_with_open_writer() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "windows_job::probe_child::tests::probe_child_fixture",
        "--ignored",
        "--nocapture",
    ]);
    let (mut child, leaked_writer) = ProbeChild::spawn_inner(&mut command, true).unwrap();
    let _leaked_writer = leaked_writer.expect("retained stdout writer handle");
    let process = child.process.try_clone().unwrap();
    let started = Instant::now();
    let result = child
        .collect_output(started + Duration::from_millis(300), 64 * 1024)
        .unwrap();
    assert!(
        result.is_none(),
        "hanging probe must return timeout even when foreign handle keeps stdout open"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "timeout must complete within bound, elapsed={:?}",
        started.elapsed()
    );
    let exited = unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
    assert_eq!(exited, WAIT_OBJECT_0, "timed-out child must be terminated");
}
