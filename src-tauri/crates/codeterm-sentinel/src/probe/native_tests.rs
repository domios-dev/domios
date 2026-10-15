use super::environment::Environment;
use super::owner::{ProbeResourceOwner, MAX_RESOURCE_SLOTS};
use super::{MAX_OUTPUT_BYTES, PROBE_RUNTIME};
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const NATIVE_TEST_RUNTIME: Duration = Duration::from_secs(30);

#[test]
fn sentinel_probe_native_success_and_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    for status in [0, 1, 2] {
        let readiness = super::test_readiness::Readiness::new();
        let (signal, receipts) = readiness.signal(PROBE_RUNTIME, PROBE_RUNTIME);
        let (output, receipt) = run_ready(
            &readiness.finite_command(status),
            dir.path(),
            PROBE_RUNTIME,
            &owner,
            &signal,
            receipts,
        )
        .unwrap_or_else(|(failure, receipt)| {
            panic!("exit {status}: {failure:?}; deadline receipt={receipt:?}")
        });
        assert_deadline_receipt(receipt, PROBE_RUNTIME, PROBE_RUNTIME);
        assert_eq!(
            i32::from_le_bytes(output.bytes[..4].try_into().unwrap()),
            status
        );
        let stdout_len = u64::from_le_bytes(output.bytes[4..12].try_into().unwrap()) as usize;
        assert_eq!(&output.bytes[12..12 + stdout_len], b"out");
        assert_eq!(&output.bytes[12 + stdout_len..], b"err");
        assert!(output.text.contains(&format!("exit={status}")));
        assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    }
}

#[test]
fn sentinel_probe_process_receives_instance_cli_and_auth_environment() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("instance-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let cli = bin.join(format!("codeterm{}", std::env::consts::EXE_SUFFIX));
    let command = if cfg!(windows) {
        std::fs::write(bin.join("codeterm.cmd"), "@echo off\r\nexit /b 0\r\n").unwrap();
        "(Get-Command codeterm).Source; Write-Output \"$env:CODETERM_API_URL|$env:CODETERM_API_PORT|$env:CODETERM_API_TOKEN|$env:CODETERM_SOCKET_PATH\""
    } else {
        let script = bin.join("codeterm");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s|%s|%s|%s' \"$CODETERM_API_URL\" \"$CODETERM_API_PORT\" \"$CODETERM_API_TOKEN\" \"$CODETERM_SOCKET_PATH\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        "command -v codeterm; codeterm"
    };
    let inherited_path = std::env::var_os("PATH");
    let environment = Environment::for_instance(
        "/instance/socket",
        "http://127.0.0.1:7686",
        "7686",
        Some("instance-token"),
        Some(cli.as_path()),
        inherited_path.as_deref(),
    );
    let (output, receipt) = super::run_with_runtime_and_receipt(
        command,
        dir.path(),
        NATIVE_TEST_RUNTIME,
        NATIVE_TEST_RUNTIME,
        &Arc::new(ProbeResourceOwner::new()),
        &environment,
    )
    .unwrap_or_else(|(failure, receipt)| {
        panic!("instance environment: {failure:?}; deadline receipt={receipt:?}")
    });
    assert_deadline_receipt(receipt, NATIVE_TEST_RUNTIME, NATIVE_TEST_RUNTIME);

    assert!(output.success, "{}", output.text);
    assert!(output
        .text
        .contains("http://127.0.0.1:7686|7686|instance-token|/instance/socket"));
    assert!(output.text.contains(bin.to_string_lossy().as_ref()));
}

#[test]
fn sentinel_probe_native_output_is_bounded_and_drained() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    let (output, receipt) = super::run_with_runtime_and_receipt(
        &large_command(),
        dir.path(),
        NATIVE_TEST_RUNTIME,
        NATIVE_TEST_RUNTIME,
        &owner,
        &Environment::default(),
    )
    .unwrap_or_else(|(failure, receipt)| {
        panic!("bounded capture: {failure:?}; deadline receipt={receipt:?}")
    });
    assert_deadline_receipt(receipt, NATIVE_TEST_RUNTIME, NATIVE_TEST_RUNTIME);
    assert_eq!(output.bytes.len(), MAX_OUTPUT_BYTES + 12);
    let stdout_len = u64::from_le_bytes(output.bytes[4..12].try_into().unwrap()) as usize;
    assert_eq!(stdout_len, 32 * 1024);
    assert!(output.bytes[12..12 + stdout_len]
        .iter()
        .all(|byte| *byte == b'x'));
    assert!(output.bytes[12 + stdout_len..]
        .iter()
        .all(|byte| *byte == b'y'));
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[cfg(unix)]
#[test]
fn sentinel_probe_native_exited_group_retirement_is_idempotent() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut child = Command::new("/bin/sh")
        .args(["-c", "IFS= read -r _; exit 0"])
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let event =
        unsafe { OwnedFd::from_raw_fd(super::cancellation::unix::parent_event(pid).unwrap()) };
    drop(child.stdin.take());
    let mut pending = libc::pollfd {
        fd: event.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(unsafe { libc::poll(&mut pending, 1, 3000) }, 1);
    assert_ne!(pending.revents & libc::POLLIN, 0);
    let ownership = super::process::LeaderOwnership::Retained(pid);
    for _ in 0..2 {
        super::group_retirement::signal_owned_group(ownership, pid).unwrap();
        super::group_retirement::retire(ownership, pid, std::time::Instant::now() + PROBE_RUNTIME)
            .unwrap();
    }
    assert!(child.wait().unwrap().success());
    assert!(super::group_retirement::observe_retired(
        super::process::LeaderOwnership::Retired(pid),
        pid
    )
    .unwrap());
    assert!(super::group_retirement::signal_owned_group(
        super::process::LeaderOwnership::Retired(pid),
        pid
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn sentinel_probe_native_closed_stream_live_child_is_unconfirmed() {
    let fixture = ControlFixture::new(true, false);
    let owner = Arc::new(ProbeResourceOwner::new());
    let result = fixture.run(&owner);
    let (failure, receipt) = result.expect_err("a live process group cannot be reported complete");
    assert_deadline_receipt(receipt.unwrap(), Duration::from_secs(1), PROBE_RUNTIME);
    assert!(receipt.unwrap().admitted.elapsed() < Duration::from_secs(3));
    assert_eq!(failure.primary, "probe_group_completion_unconfirmed");
    assert!(failure.is_unconfirmed());
    assert!(failure
        .unconfirmed
        .contains(&super::process::ResourceKind::ProcessGroup));
    assert!(owner.wait_for_unconfirmed_with_timeout(Duration::from_secs(3)));
    assert_eq!(owner.snapshot().unconfirmed, 1);
    fixture.release_and_wait();
}

#[cfg(unix)]
#[test]
fn sentinel_probe_native_inherited_stream_deadline() {
    let fixture = ControlFixture::new(false, true);
    let owner = Arc::new(ProbeResourceOwner::new());
    let result = fixture.run(&owner);
    let (failure, receipt) = result.expect_err("inherited output keeps capture incomplete");
    assert_deadline_receipt(receipt.unwrap(), Duration::from_secs(1), PROBE_RUNTIME);
    assert!(receipt.unwrap().admitted.elapsed() >= Duration::from_millis(500));
    assert!(receipt.unwrap().admitted.elapsed() < Duration::from_secs(3));
    fixture.release_and_wait();
    assert_eq!(failure.primary, "probe_timeout");
    assert!(failure.is_unconfirmed());
    assert!(failure
        .unconfirmed
        .contains(&super::process::ResourceKind::Capture));
    assert!(owner.wait_for_free_slots_with_timeout(MAX_RESOURCE_SLOTS, Duration::from_secs(3)));
}

#[cfg(unix)]
#[test]
fn sentinel_probe_timeout_retires_sigterm_ignoring_child_and_reuses_owner() {
    let mut fixture = ControlFixture::new(false, false);
    fixture.ignore_term = true;
    let owner = Arc::new(ProbeResourceOwner::new());
    let (failure, receipt) = fixture
        .run(&owner)
        .expect_err("SIGTERM-ignoring child must reach the probe deadline");
    assert_deadline_receipt(receipt.unwrap(), Duration::from_secs(1), PROBE_RUNTIME);
    assert!(fixture.dir.path().join("term-ignored").exists());
    assert_eq!(failure.primary, "probe_timeout");
    assert!(failure.processes_confirmed, "{failure:?}");
    assert!(!failure.is_unconfirmed(), "{failure:?}");
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    let recovered = super::run(
        "printf recovered",
        fixture.dir.path(),
        PROBE_RUNTIME,
        &owner,
    )
    .unwrap();
    assert!(recovered.success);
    assert!(recovered.text.contains("recovered"));
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[cfg(windows)]
#[test]
fn sentinel_probe_native_windows_preexecution_containment() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    let marker = dir.path().join("probe-started.txt");
    let command = format!(
        "[IO.File]::WriteAllText('{}', 'started'); exit 0",
        marker.display()
    );
    let deadlines = super::process::Deadlines::start(PROBE_RUNTIME, PROBE_RUNTIME).unwrap();
    let receipt = deadlines.receipt();
    let result = super::windows::run_with_assignment(
        &command,
        dir.path(),
        deadlines,
        owner.reserve().unwrap(),
        &owner,
        &super::cancellation::Cancellation::default(),
        &Environment::default(),
        |_, _| Err("injected Job assignment failure".into()),
    );
    assert_deadline_receipt(receipt, PROBE_RUNTIME, PROBE_RUNTIME);
    let failure = result.expect_err("assignment failure must stop before resume");
    assert_eq!(failure.primary, "probe_containment_failed");
    assert!(failure
        .cleanup_errors
        .iter()
        .any(|error| error == "injected Job assignment failure"));
    assert!(!failure.is_unconfirmed());
    assert!(!marker.exists());
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[cfg(not(windows))]
#[test]
#[ignore = "Windows Job containment oracle; not executed on this platform"]
fn sentinel_probe_native_windows_preexecution_containment() {}

#[cfg(windows)]
#[test]
fn sentinel_probe_native_cancelled_capture_settles() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    let result = super::run_with_receipt(
        "while ($true) { [Console]::Out.Write('x' * 4096) }",
        dir.path(),
        Duration::from_secs(3),
        &owner,
    );
    let (failure, receipt) = result.expect_err("cancelled Windows capture is not a normal result");
    assert_deadline_receipt(receipt.unwrap(), Duration::from_secs(3), PROBE_RUNTIME);
    assert_eq!(failure.primary, "probe_timeout");
    assert!(!failure.is_unconfirmed());
    assert!(owner.wait_for_free_slots_with_timeout(MAX_RESOURCE_SLOTS, Duration::from_secs(5)));
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[cfg(not(windows))]
#[test]
#[ignore = "Windows overlapped cancellation oracle; not executed on this platform"]
fn sentinel_probe_native_cancelled_capture_settles() {}

#[cfg(unix)]
#[test]
#[ignore = "Launched only as a controlled descendant by native probe fixtures"]
fn probe_native_control_child() {
    let Ok(control) = std::env::var("SENTINEL_PROBE_NATIVE_CONTROL") else {
        return;
    };
    let ack = std::env::var("SENTINEL_PROBE_NATIVE_ACK").unwrap();
    let started = std::env::var("SENTINEL_PROBE_NATIVE_STARTED").unwrap();
    let detached = std::env::var_os("SENTINEL_PROBE_NATIVE_DETACH").is_some();
    let close_streams = std::env::var_os("SENTINEL_PROBE_NATIVE_CLOSE_STREAMS").is_some();
    if std::env::var_os("SENTINEL_PROBE_NATIVE_IGNORE_TERM").is_some() {
        assert_ne!(
            unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) },
            libc::SIG_ERR
        );
        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        std::fs::write(
            std::env::var("SENTINEL_PROBE_NATIVE_TERM_MARKER").unwrap(),
            b"ignored",
        )
        .unwrap();
    }
    if detached {
        assert_eq!(unsafe { libc::setsid() }, std::process::id() as i32);
    }
    if close_streams {
        unsafe {
            libc::close(libc::STDOUT_FILENO);
            libc::close(libc::STDERR_FILENO);
        }
    }
    if let Ok(address) = std::env::var("SENTINEL_FIXTURE_READY") {
        super::test_readiness::child_ready(&address);
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(started)
        .unwrap()
        .write_all(b"ready\n")
        .unwrap();
    let mut release = std::io::BufReader::new(std::fs::File::open(control).unwrap());
    let mut line = String::new();
    std::io::BufRead::read_line(&mut release, &mut line).unwrap();
    assert_eq!(line, "release\n");
    std::fs::OpenOptions::new()
        .write(true)
        .open(ack)
        .unwrap()
        .write_all(b"complete\n")
        .unwrap();
}

#[cfg(unix)]
struct ControlFixture {
    dir: tempfile::TempDir,
    control: std::fs::File,
    ack: std::fs::File,
    control_path: std::path::PathBuf,
    ack_path: std::path::PathBuf,
    close_streams: bool,
    detached: bool,
    ignore_term: bool,
    readiness: Arc<super::test_readiness::Readiness>,
}

#[cfg(unix)]
impl ControlFixture {
    fn new(close_streams: bool, detached: bool) -> Self {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let control_path = dir.path().join("control.fifo");
        let ack_path = dir.path().join("ack.fifo");
        let started = dir.path().join("started.fifo");
        for path in [&control_path, &ack_path, &started] {
            let path_c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) }, 0);
        }
        let control = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&control_path)
            .unwrap();
        let ack = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&ack_path)
            .unwrap();
        Self {
            dir,
            control,
            ack,
            control_path,
            ack_path,
            close_streams,
            detached,
            ignore_term: false,
            readiness: super::test_readiness::Readiness::new(),
        }
    }

    fn command(&self) -> String {
        let executable = std::env::current_exe().unwrap();
        format!(
            concat!(
                "SENTINEL_PROBE_NATIVE_CONTROL={} SENTINEL_PROBE_NATIVE_ACK={} ",
                "SENTINEL_PROBE_NATIVE_STARTED={}{}{}{} SENTINEL_FIXTURE_READY={} {} ",
                "--ignored --nocapture probe_native_control_child & ",
                "IFS= read -r _ < {}; exit 0"
            ),
            shell_quote(&self.control_path),
            shell_quote(&self.ack_path),
            shell_quote(&self.dir.path().join("started.fifo")),
            if self.detached {
                " SENTINEL_PROBE_NATIVE_DETACH=1"
            } else {
                ""
            },
            if self.close_streams {
                " SENTINEL_PROBE_NATIVE_CLOSE_STREAMS=1"
            } else {
                ""
            },
            if self.ignore_term {
                format!(
                    " SENTINEL_PROBE_NATIVE_IGNORE_TERM=1 SENTINEL_PROBE_NATIVE_TERM_MARKER={}",
                    shell_quote(&self.dir.path().join("term-ignored"))
                )
            } else {
                String::new()
            },
            self.readiness.address(),
            shell_quote(&executable),
            shell_quote(&self.dir.path().join("started.fifo")),
        )
    }

    fn run(
        &self,
        owner: &Arc<ProbeResourceOwner>,
    ) -> Result<
        (super::Output, super::process::DeadlineReceipt),
        (
            super::process::ProbeFailure,
            Option<super::process::DeadlineReceipt>,
        ),
    > {
        let limit = Duration::from_secs(1);
        let (signal, receipts) = self.readiness.signal(limit, PROBE_RUNTIME);
        run_ready(
            &self.command(),
            self.dir.path(),
            limit,
            owner,
            &signal,
            receipts,
        )
    }

    fn release_and_wait(&self) {
        use std::io::Write;
        let mut control = &self.control;
        control.write_all(b"release\n").unwrap();
        let mut poll = libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(&self.ack),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let deadline_ms = NATIVE_TEST_RUNTIME.as_millis() as libc::c_int;
        assert_eq!(unsafe { libc::poll(&mut poll, 1, deadline_ms) }, 1);
        let mut ack = [0u8; 32];
        let read = unsafe {
            libc::read(
                std::os::fd::AsRawFd::as_raw_fd(&self.ack),
                ack.as_mut_ptr().cast(),
                ack.len(),
            )
        };
        assert_eq!(&ack[..read as usize], b"complete\n");
    }
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn large_command() -> String {
    "head -c 100000 /dev/zero | tr '\\000' x; head -c 100000 /dev/zero | tr '\\000' y >&2".into()
}

#[cfg(windows)]
fn large_command() -> String {
    "[Console]::Out.Write('x' * 100000); [Console]::Error.Write('y' * 100000)".into()
}

fn assert_deadline_receipt(
    receipt: super::process::DeadlineReceipt,
    limit: Duration,
    runtime: Duration,
) {
    let duration = limit.min(runtime);
    let reserve = Duration::from_millis(500).min(duration / 4);
    assert_eq!(receipt.duration, duration);
    assert_eq!(receipt.hard.duration_since(receipt.admitted), duration);
    assert_eq!(
        receipt.operation.duration_since(receipt.admitted),
        duration - reserve
    );
}

fn run_ready(
    command: &str,
    cwd: &std::path::Path,
    limit: Duration,
    owner: &Arc<ProbeResourceOwner>,
    signal: &super::cancellation::Cancellation,
    receipts: std::sync::mpsc::Receiver<super::process::DeadlineReceipt>,
) -> Result<
    (super::Output, super::process::DeadlineReceipt),
    (
        super::process::ProbeFailure,
        Option<super::process::DeadlineReceipt>,
    ),
> {
    let result = super::run_transaction(
        command,
        cwd,
        super::process::Deadlines::start(limit, PROBE_RUNTIME).unwrap(),
        owner,
        signal,
        false,
        &Environment::default(),
    );
    let receipt = receipts.try_recv().ok();
    result
        .map(|output| (output, receipt.expect("child readiness receipt")))
        .map_err(|failure| (failure, receipt))
}
