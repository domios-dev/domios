use std::sync::Arc;
use std::time::{Duration, Instant};

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
        #[cfg(unix)]
        let command = format!("printf out; printf err >&2; exit {code}");
        #[cfg(windows)]
        let command =
            format!("[Console]::Out.Write('out'); [Console]::Error.Write('err'); exit {code}");
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
            &Cancellation::default(),
        );
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
    let fixture = Fixture::new(close_streams);
    let owner = Arc::new(ProbeResourceOwner::new());
    let signal = Cancellation::default();
    let waits = signal.observe_waits();
    let parameters = Parameters {
        command: fixture.command(),
        label: "Controlled child".into(),
        timeout_ms: probe::PROBE_RUNTIME.as_millis() as u64,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let worker_owner = owner.clone();
    let worker_signal = signal.clone();
    let cwd = fixture.cwd();
    let started = Instant::now();
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
    fixture.wait_ready();
    let expected = if close_streams {
        #[cfg(unix)]
        let phase = WaitPhase::Parent;
        #[cfg(windows)]
        let phase = WaitPhase::ParentAfterEof;
        phase
    } else {
        WaitPhase::Capture
    };
    let deadline = Instant::now() + probe::PROBE_RUNTIME;
    let mut last_native_state = None;
    loop {
        let phase = match waits.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(phase) => phase,
            Err(error) => {
                signal.cancel();
                let completed = rx.try_recv().ok().and_then(|effect| effect.command_result);
                panic!("native wait did not enroll before failure deadline: {error:?}; last_native_state={last_native_state:?}; completed_result={completed:?}");
            }
        };
        #[cfg(windows)]
        if matches!(phase, WaitPhase::WindowsCapture { .. }) {
            last_native_state = Some(phase);
        }
        #[cfg(unix)]
        {
            last_native_state = Some(phase);
        }
        if phase == expected {
            break;
        }
    }
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

    fn wait_ready(&self) {
        use std::os::fd::AsRawFd;
        let mut fd = libc::pollfd {
            fd: self.ready.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(
            unsafe { libc::poll(&mut fd, 1, probe::PROBE_RUNTIME.as_millis() as i32) },
            1,
            "child readiness was not observed"
        );
        let mut buffer = [0u8; 32];
        let length = unsafe {
            libc::read(
                self.ready.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        assert_eq!(&buffer[..length as usize], b"ready\n");
    }
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
        let close = if self.close_streams {
            r#"Add-Type -ErrorAction Stop -TypeDefinition 'using System; using System.ComponentModel; using System.Runtime.InteropServices; public static class CaptureFixture { [DllImport("kernel32.dll", SetLastError=true)] private static extern IntPtr GetStdHandle(int n); [DllImport("kernel32.dll", SetLastError=true)] private static extern uint GetFileType(IntPtr h); [DllImport("kernel32.dll", SetLastError=true)] private static extern bool CloseHandle(IntPtr h); public static void CloseStreams() { IntPtr output=GetStdHandle(-11), error=GetStdHandle(-12); if (GetFileType(output)!=3 || GetFileType(error)!=3 || output==error) throw new InvalidOperationException("fixture stdout/stderr must be distinct pipe handles"); if (!CloseHandle(output)) throw new Win32Exception(Marshal.GetLastWin32Error(), "fixture stdout close failed"); if (!CloseHandle(error)) throw new Win32Exception(Marshal.GetLastWin32Error(), "fixture stderr close failed"); } }'; [CaptureFixture]::CloseStreams();"#
        } else {
            "[Console]::Out.Write('out'); [Console]::Error.Write('err');"
        };
        format!("$ErrorActionPreference='Stop'; $ready=[Threading.EventWaitHandle]::OpenExisting('{}'); $release=[Threading.EventWaitHandle]::OpenExisting('{}'); try {{ {close} $ready.Set() | Out-Null; if (!$release.WaitOne(10000)) {{ exit 13 }}; exit 0 }} catch {{ $message=$_.Exception.Message; [IO.File]::WriteAllText('fixture-error.txt',$message.Substring(0,[Math]::Min(1024,$message.Length))); $ready.Set() | Out-Null; exit 14 }}", self.ready_name, self.release_name)
    }

    fn wait_ready(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
        };
        assert_eq!(
            unsafe {
                WaitForSingleObject(
                    self.ready.as_raw_handle(),
                    probe::PROBE_RUNTIME.as_millis() as u32,
                )
            },
            WAIT_OBJECT_0,
            "child readiness was not observed"
        );
        let failure = self.dir.path().join("fixture-error.txt");
        assert!(
            !failure.exists(),
            "fixture readiness failed: {}",
            std::fs::read_to_string(failure).unwrap_or_else(|error| error.to_string())
        );
    }
}
