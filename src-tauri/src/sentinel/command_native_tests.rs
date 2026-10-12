use std::sync::Arc;
use std::time::Instant;

use super::command::{self, Parameters, Status};
use super::handlers::Status as HandlerStatus;
use super::probe::{
    self,
    cancellation::{Cancellation, WaitPhase},
    owner::{ProbeResourceOwner, MAX_RESOURCE_SLOTS},
};

#[test]
fn sentinel_command_native_finite_actual_zero_nonzero_stdout_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    for code in [0, 7] {
        let readiness = probe::test_readiness::Readiness::new();
        let (signal, receipts) = readiness.signal(probe::PROBE_RUNTIME, probe::PROBE_RUNTIME);
        let command = readiness.finite_command(code);
        let parameters = Parameters {
            command,
            label: "Finite fixture".into(),
            timeout_ms: probe::PROBE_RUNTIME.as_millis() as u64,
        };
        let effect = command::execute(
            &parameters,
            dir.path(),
            probe::PROBE_RUNTIME,
            &owner,
            &signal,
        );
        receipts.try_recv().expect("finite child readiness receipt");
        let metadata = effect.command_result.unwrap();
        assert_eq!(
            metadata.status,
            if code == 0 {
                Status::Success
            } else {
                Status::Nonzero
            },
            "{metadata:?}"
        );
        assert_eq!(metadata.exit_code, Some(code));
        assert!(metadata.resources_confirmed);
        assert_eq!(metadata.stdout.unwrap().total_bytes, 3);
        assert_eq!(metadata.stderr.unwrap().total_bytes, 3);
        assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    }
}

#[test]
fn sentinel_command_native_precancel_refuses_before_side_effect_and_owner_admission() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    let signal = Cancellation::default();
    signal.cancel();
    let parameters = Parameters {
        command: "invalid-command-must-not-be-launched".into(),
        label: "Precancelled".into(),
        timeout_ms: 1000,
    };
    let effect = command::execute(
        &parameters,
        dir.path(),
        probe::PROBE_RUNTIME,
        &owner,
        &signal,
    );
    let result = effect.command_result.unwrap();
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!(result.exit_code, None);
    assert_eq!(result.stdout, None);
    assert!(result.resources_confirmed);
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[test]
fn sentinel_command_native_cancel_during_capture_settles_exact_owned_resources() {
    cancellation_control(false, false);
}

#[test]
fn sentinel_command_native_cancel_after_eof_wakes_parent_wait_and_settles() {
    cancellation_control(true, false);
}

#[test]
fn sentinel_command_native_owner_shutdown_wakes_active_wait() {
    cancellation_control(false, true);
}

fn cancellation_control(close_streams: bool, shutdown: bool) {
    let fixture = Arc::new(Fixture::new(close_streams));
    let owner = Arc::new(ProbeResourceOwner::new());
    #[cfg(unix)]
    let signal = Cancellation::default();
    #[cfg(windows)]
    let signal = fixture.cancellation();
    let waits = Arc::new(std::sync::Mutex::new(signal.observe_waits()));
    let ready_fixture = fixture.clone();
    let ready_waits = waits.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    // Windows emits capture waits while pipes connect, before the child exists; the probe
    // thread drains them here, before drive() can emit a post-readiness wait.
    signal.set_readiness(move || {
        ready_fixture.wait_ready()?;
        let deadlines =
            probe::process::Deadlines::start(probe::PROBE_RUNTIME, probe::PROBE_RUNTIME)
                .map_err(|error| error.to_string())?;
        let early: Vec<WaitPhase> = ready_waits.lock().unwrap().try_iter().collect();
        let _ = ready_tx.send(early);
        Ok(deadlines)
    });
    let parameters = Parameters {
        command: fixture.command(),
        label: "Controlled child".into(),
        timeout_ms: probe::PROBE_RUNTIME.as_millis() as u64,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let worker_owner = owner.clone();
    let worker_signal = signal.clone();
    let cwd = fixture.cwd();
    let worker = std::thread::spawn(move || {
        let result = command::execute(
            &parameters,
            &cwd,
            probe::PROBE_RUNTIME,
            &worker_owner,
            &worker_signal,
        );
        tx.send(result).unwrap();
    });
    let expected = if close_streams {
        #[cfg(unix)]
        let phase = WaitPhase::Parent;
        #[cfg(windows)]
        let phase = WaitPhase::ParentAfterEof;
        phase
    } else {
        WaitPhase::Capture
    };
    let deadline = Instant::now() + probe::test_readiness::SETUP_DEADLINE;
    let mut last_native_state = None;
    #[cfg(windows)]
    let mut native_diagnostics = WindowsDiagnostics::default();
    let Ok(_early) = ready_rx.recv_timeout(probe::test_readiness::SETUP_DEADLINE) else {
        signal.cancel();
        panic!("controlled child readiness was not observed before the failure deadline");
    };
    #[cfg(windows)]
    for phase in _early {
        native_diagnostics.observe(phase);
    }
    let waits = waits.lock().unwrap();
    loop {
        let phase = match waits.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(phase) => phase,
            Err(error) => {
                signal.cancel();
                let completed = rx.try_recv().ok().and_then(|effect| effect.command_result);
                #[cfg(windows)]
                let diagnostic = format!("{native_diagnostics:?}");
                #[cfg(unix)]
                let diagnostic = "";
                panic!("native wait did not enroll before failure deadline: {error:?}; last_native_state={last_native_state:?}; first_transitions={diagnostic}; completed_result={completed:?}");
            }
        };
        #[cfg(windows)]
        {
            native_diagnostics.observe(phase);
            if matches!(phase, WaitPhase::WindowsCapture { .. }) {
                last_native_state = Some(phase);
            }
        }
        #[cfg(unix)]
        {
            last_native_state = Some(phase);
        }
        if phase == expected {
            break;
        }
    }
    let started = Instant::now();
    if shutdown {
        owner.close_admission();
    } else {
        signal.cancel();
    }
    let effect = rx
        .recv_timeout(probe::PROBE_RUNTIME)
        .expect("cancellation failed to settle before bounded deadline");
    worker.join().unwrap();
    let metadata = effect.command_result.unwrap();
    assert_eq!(effect.status, HandlerStatus::Failed, "{metadata:?}");
    assert_eq!(metadata.status, Status::Cancelled, "{metadata:?}");
    assert!(metadata.resources_confirmed, "{metadata:?}");
    assert!(metadata.stdout.is_some());
    assert!(metadata.stderr.is_some());
    assert!(started.elapsed() < probe::PROBE_RUNTIME * 2);
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

#[cfg(windows)]
#[derive(Default, Debug)]
struct WindowsDiagnostics {
    last_running_state: Option<WaitPhase>,
    first_native_error: Option<WaitPhase>,
    transitions: Vec<WaitPhase>,
}

#[cfg(windows)]
impl WindowsDiagnostics {
    fn observe(&mut self, phase: WaitPhase) {
        match phase {
            WaitPhase::WindowsCapture {
                cancel_attempted: false,
                ..
            } => self.last_running_state = Some(phase),
            WaitPhase::WindowsIo { error, .. } => {
                if self.first_native_error.is_none()
                    && error.is_some_and(|error| {
                        !matches!(
                            error,
                            windows_sys::Win32::Foundation::ERROR_IO_PENDING
                                | windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE
                        )
                    })
                {
                    self.first_native_error = Some(phase);
                }
                if self.transitions.len() < 33 {
                    self.transitions.push(phase);
                }
            }
            _ => {}
        }
    }
}

#[cfg(unix)]
struct Fixture {
    dir: tempfile::TempDir,
    ready: std::fs::File,
    _release: std::fs::File,
    close_streams: bool,
}

#[cfg(unix)]
impl Fixture {
    fn new(close_streams: bool) -> Self {
        use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
        let dir = tempfile::tempdir().unwrap();
        for name in ["ready", "release"] {
            let path =
                std::ffi::CString::new(dir.path().join(name).as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        }
        let open = |name| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(dir.path().join(name))
                .unwrap()
        };
        let ready = open("ready");
        let release = open("release");
        Self {
            dir,
            ready,
            _release: release,
            close_streams,
        }
    }

    fn cwd(&self) -> std::path::PathBuf {
        self.dir.path().to_path_buf()
    }

    fn command(&self) -> String {
        let quote =
            |path: &std::path::Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        format!("SENTINEL_PROBE_NATIVE_CONTROL={} SENTINEL_PROBE_NATIVE_ACK={} SENTINEL_PROBE_NATIVE_STARTED={} {} exec {} --exact --ignored --nocapture sentinel::probe::native_tests::probe_native_control_child", quote(&self.dir.path().join("release")), quote(&self.dir.path().join("ready")), quote(&self.dir.path().join("ready")), if self.close_streams { "SENTINEL_PROBE_NATIVE_CLOSE_STREAMS=1" } else { "" }, quote(&std::env::current_exe().unwrap()))
    }

    fn wait_ready(&self) -> Result<(), String> {
        use std::os::fd::AsRawFd;
        let mut fd = libc::pollfd {
            fd: self.ready.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe {
            libc::poll(
                &mut fd,
                1,
                probe::test_readiness::SETUP_DEADLINE.as_millis() as i32,
            )
        } != 1
        {
            return Err("child readiness was not observed".into());
        }
        let mut buffer = [0u8; 32];
        let length = unsafe {
            libc::read(
                self.ready.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if length != 6 || &buffer[..6] != b"ready\n" {
            return Err("invalid child readiness marker".into());
        }
        Ok(())
    }
}

#[cfg(windows)]
const WINDOWS_CHILD_SELECTOR: &str =
    "sentinel::command_native_tests::windows_controlled_capture_child";

#[cfg(windows)]
#[test]
#[ignore = "Launched only as the exact owned child by the Windows post-EOF control"]
fn windows_controlled_capture_child() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::SetEvent;

    let mut ready = None;
    let result = windows_capture_child(&mut ready);
    let code = if let Err(error) = result {
        let bounded = error.chars().take(1024).collect::<String>();
        if std::fs::write("fixture-error.txt", bounded).is_ok() {
            if let Some(ready) = &ready {
                unsafe { SetEvent(ready.as_raw_handle()) };
            }
        }
        14
    } else {
        0
    };
    drop(ready);
    std::process::exit(code);
}

#[cfg(windows)]
fn windows_capture_child(
    ready: &mut Option<std::os::windows::io::OwnedHandle>,
) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{
            SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
        },
    };

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 9
        || args[1..6]
            != [
                "--exact",
                "--ignored",
                "--nocapture",
                WINDOWS_CHILD_SELECTOR,
                "--skip",
            ]
        || args[7] != "--skip"
    {
        return Err(
            "native child requires its exact selector and two owned event arguments".into(),
        );
    }
    let id = args[6]
        .strip_prefix("Local\\sentinel-ready-")
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .ok_or("native child ready event identity is invalid")?;
    if args[6] != format!("Local\\sentinel-ready-{id}")
        || args[8] != format!("Local\\sentinel-release-{id}")
    {
        return Err("native child event identities do not match".into());
    }
    *ready = Some(open_child_event(&args[6], EVENT_MODIFY_STATE)?);
    let release = open_child_event(&args[8], SYNCHRONIZATION_SYNCHRONIZE)?;
    close_child_pipes()?;
    if unsafe { SetEvent(ready.as_ref().unwrap().as_raw_handle()) } == 0 {
        return Err(child_native_error("SetEvent ready"));
    }
    match unsafe {
        WaitForSingleObject(
            release.as_raw_handle(),
            probe::PROBE_RUNTIME.as_millis() as u32,
        )
    } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => {
            Err("native child release event did not signal before its failure deadline".into())
        }
        _ => Err(child_native_error("WaitForSingleObject release")),
    }
}

#[cfg(windows)]
fn open_child_event(name: &str, access: u32) -> Result<std::os::windows::io::OwnedHandle, String> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Threading::OpenEventW;
    let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
    let raw = unsafe { OpenEventW(access, 0, wide.as_ptr()) };
    if raw.is_null() {
        Err(child_native_error("OpenEventW"))
    } else {
        Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) })
    }
}

#[cfg(windows)]
fn close_child_pipes() -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE},
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(handle: u32) -> HANDLE;
    }
    let stdout = unsafe { GetStdHandle((-11i32) as u32) };
    let stderr = unsafe { GetStdHandle((-12i32) as u32) };
    if stdout.is_null()
        || stderr.is_null()
        || stdout == INVALID_HANDLE_VALUE
        || stderr == INVALID_HANDLE_VALUE
        || stdout == stderr
        || unsafe { GetFileType(stdout) } != FILE_TYPE_PIPE
        || unsafe { GetFileType(stderr) } != FILE_TYPE_PIPE
    {
        return Err("native child stdout/stderr must be distinct owned pipe handles".into());
    }
    if unsafe { CloseHandle(stdout) } == 0 {
        return Err(child_native_error("CloseHandle stdout"));
    }
    if unsafe { CloseHandle(stderr) } == 0 {
        return Err(child_native_error("CloseHandle stderr"));
    }
    Ok(())
}

#[cfg(windows)]
fn child_native_error(operation: &str) -> String {
    format!("{operation} win32_error={}", unsafe {
        windows_sys::Win32::Foundation::GetLastError()
    })
}

#[cfg(windows)]
struct Fixture {
    dir: tempfile::TempDir,
    ready: std::os::windows::io::OwnedHandle,
    _release: std::os::windows::io::OwnedHandle,
    ready_name: String,
    release_name: String,
    close_streams: bool,
}

#[cfg(windows)]
impl Fixture {
    fn new(close_streams: bool) -> Self {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::System::Threading::CreateEventW;
        let id = uuid::Uuid::new_v4();
        let ready_name = format!("Local\\sentinel-ready-{id}");
        let release_name = format!("Local\\sentinel-release-{id}");
        let open = |name: &str| {
            let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
            let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
            assert!(!raw.is_null(), "fixture event creation failed");
            unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) }
        };
        Self {
            dir: tempfile::tempdir().unwrap(),
            ready: open(&ready_name),
            _release: open(&release_name),
            ready_name,
            release_name,
            close_streams,
        }
    }

    fn cwd(&self) -> std::path::PathBuf {
        self.dir.path().to_path_buf()
    }

    fn command(&self) -> String {
        if self.close_streams {
            return format!(
                "& {}",
                self.native_argv()
                    .iter()
                    .map(|argument| { format!("'{}'", argument.replace('\'', "''")) })
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        let close = "[Console]::Out.Write('out'); [Console]::Error.Write('err');";
        format!("$ErrorActionPreference='Stop'; $ready=[Threading.EventWaitHandle]::OpenExisting('{}'); $release=[Threading.EventWaitHandle]::OpenExisting('{}'); try {{ {close} $ready.Set() | Out-Null; if (!$release.WaitOne(10000)) {{ exit 13 }}; exit 0 }} catch {{ $message=$_.Exception.Message; [IO.File]::WriteAllText('fixture-error.txt',$message.Substring(0,[Math]::Min(1024,$message.Length))); $ready.Set() | Out-Null; exit 14 }}", self.ready_name, self.release_name)
    }

    fn native_argv(&self) -> Vec<String> {
        vec![
            std::env::current_exe()
                .unwrap()
                .into_os_string()
                .into_string()
                .expect("native fixture executable path must be Unicode"),
            "--exact".into(),
            "--ignored".into(),
            "--nocapture".into(),
            WINDOWS_CHILD_SELECTOR.into(),
            "--skip".into(),
            self.ready_name.clone(),
            "--skip".into(),
            self.release_name.clone(),
        ]
    }

    fn cancellation(&self) -> Cancellation {
        if self.close_streams {
            Cancellation::for_windows_fixture(self.native_argv())
        } else {
            Cancellation::default()
        }
    }

    fn wait_ready(&self) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
        };
        let failure = self.dir.path().join("fixture-error.txt");
        let observed = unsafe {
            WaitForSingleObject(
                self.ready.as_raw_handle(),
                probe::test_readiness::SETUP_DEADLINE.as_millis() as u32,
            )
        };
        if observed != WAIT_OBJECT_0 || failure.exists() {
            return Err(format!(
                "child readiness failed: {}",
                std::fs::read_to_string(failure).unwrap_or_else(|error| error.to_string())
            ));
        }
        Ok(())
    }
}
