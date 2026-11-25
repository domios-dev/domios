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
    println!("\nopaque-cli 0.172.0");
    std::io::stdout().flush().unwrap();
}

fn read_until_line(
    port: HANDLE,
    stdout: OwnedHandle,
    needle: &str,
    timeout: Duration,
) -> io::Result<bool> {
    let mut reader = IocpReader::new(port, stdout);
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    let limit: usize = 64 * 1024;
    loop {
        if reader.is_idle() && reader.start_read(limit.saturating_sub(output.len()))? {
            reader.close();
            return Ok(String::from_utf8_lossy(&output)
                .lines()
                .any(|line| line == needle));
        }
        let now = Instant::now();
        if now >= deadline {
            reader.close();
            return Ok(false);
        }
        let wait_ms = (deadline
            .saturating_duration_since(now)
            .as_millis()
            .min(u128::from(u32::MAX - 1)) as u32)
            .max(1);
        let mut bytes = 0u32;
        let mut key = 0usize;
        let mut overlapped: *mut OVERLAPPED = null_mut();
        let ok = unsafe {
            GetQueuedCompletionStatus(port, &mut bytes, &mut key, &mut overlapped, wait_ms)
        };
        if ok == 0 && overlapped.is_null() {
            let err = unsafe { GetLastError() };
            reader.close();
            if err == WAIT_TIMEOUT {
                return Ok(false);
            }
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        if key == JOB_KEY {
            continue;
        }
        if key == STDOUT_KEY {
            let io_error = (ok == 0).then(|| unsafe { GetLastError() });
            let done = reader.on_stdout_completion(overlapped, bytes, io_error, &mut output)?;
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
    let ready = read_until_line(
        owner.port.as_raw_handle(),
        stdout,
        "probe-child-ready",
        Duration::from_secs(5),
    );
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
            .any(|line| line.trim() == "opaque-cli 0.172.0"),
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

#[test]
fn iocp_reader_retains_overlapped_storage_when_cancel_does_not_settle() {
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
    assert!(!port.is_null());
    let port = unsafe { OwnedHandle::from_raw_handle(port) };

    let (pipe_reader, _pipe_writer) = create_overlapped_pipe().unwrap();
    let unsettled_dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut reader = IocpReader::new(port.as_raw_handle(), pipe_reader);
    reader.cancel_timeout_ms = 0;
    reader.storage.as_deref_mut().unwrap().dropped = Some(unsettled_dropped.clone());
    reader.state = ReadState::Active;
    reader.close();
    assert_eq!(reader.state, ReadState::Unsettled);
    assert!(
        reader.pipe.is_some(),
        "unsettled close must keep the pipe handle intact"
    );
    drop(reader);
    assert!(
        !unsettled_dropped.load(Ordering::SeqCst),
        "unsettled OVERLAPPED and read buffer must be forgotten, not freed"
    );

    let (pipe_reader, _pipe_writer) = create_overlapped_pipe().unwrap();
    assert!(!unsafe {
        CreateIoCompletionPort(
            pipe_reader.as_raw_handle(),
            port.as_raw_handle(),
            STDOUT_KEY,
            0,
        )
    }
    .is_null());
    let settled_dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut reader = IocpReader::new(port.as_raw_handle(), pipe_reader);
    reader.storage.as_deref_mut().unwrap().dropped = Some(settled_dropped.clone());
    assert!(!reader.start_read(64).unwrap());
    assert_eq!(reader.state, ReadState::Active);
    reader.close();
    assert_eq!(reader.state, ReadState::Idle);
    assert!(reader.pipe.is_none());
    drop(reader);
    assert!(
        settled_dropped.load(Ordering::SeqCst),
        "settled cancel must release OVERLAPPED and read buffer"
    );
}
