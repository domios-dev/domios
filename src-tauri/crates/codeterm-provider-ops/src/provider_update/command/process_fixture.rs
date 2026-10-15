//! Real updater children use socket gates; independent OS exit handles verify fixture cleanup.
//! Parent exit and closed output must never substitute for remaining-work completion.
//! pidfd, kqueue and Windows process handles independently receipt each fixture exit.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::{EnvironmentPolicy, Invocation, UpdateCommandFailure};

const ENTRY: &str = "provider_update::command::process_fixture::provider_update_fixture_entry";
const BOUND: Duration = Duration::from_secs(15);

pub(super) fn invocation(mode: &str, address: &str) -> Invocation {
    #[allow(unused_mut)]
    let mut environment = vec![
        ("COURIER_FIXTURE_MODE".into(), mode.into()),
        ("COURIER_FIXTURE_ADDRESS".into(), address.into()),
    ];
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        environment.push(("SystemRoot".into(), root));
    }
    Invocation {
        executable: std::env::current_exe().unwrap(),
        args: vec![ENTRY.into(), "--exact".into(), "--nocapture".into()],
        cwd: Some(std::env::temp_dir()),
        environment: EnvironmentPolicy::Explicit(environment),
        execution: BOUND,
        cleanup: Duration::from_secs(1),
        controls: super::ExecutorControls::default(),
    }
}

pub(super) struct Fixture {
    listener: TcpListener,
    address: String,
    peers: Vec<Peer>,
    expire: Option<mpsc::Sender<()>>,
    reaped: Option<Receiver<()>>,
    continue_parent: Option<mpsc::Sender<()>>,
    pub(super) result: Receiver<Result<(), UpdateCommandFailure>>,
}

struct Peer {
    role: String,
    socket: TcpStream,
    exit: ExitReceipt,
}

impl Fixture {
    pub(super) fn start(mode: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mut invocation = invocation(
            if mode == "deadline" || mode == "termination" {
                "blocked"
            } else {
                mode
            },
            &address,
        );
        let expire = if mode == "deadline" || mode == "termination" {
            let (tx, rx) = mpsc::channel();
            invocation.controls.deadline_gate = Some(rx);
            invocation.controls.termination_error = mode == "termination";
            Some(tx)
        } else {
            None
        };
        let (reaped, continue_parent) =
            if mode == "closed" || mode == "inherited" || mode == "windows-wait" {
                let (event_tx, event_rx) = mpsc::channel();
                let (gate_tx, gate_rx) = mpsc::channel();
                invocation.controls.parent_event = Some(event_tx);
                invocation.controls.parent_continue = Some(gate_rx);
                invocation.controls.expire_capture =
                    mode == "inherited" || (mode == "closed" && cfg!(windows));
                (Some(event_rx), Some(gate_tx))
            } else {
                (None, None)
            };
        let (tx, result) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(super::run_invocation(invocation));
        });
        let mut fixture = Self {
            listener,
            address,
            peers: Vec::new(),
            expire,
            reaped,
            continue_parent,
            result,
        };
        let count = if mode == "blocked" || mode == "deadline" || mode == "termination" {
            1
        } else {
            2
        };
        for _ in 0..count {
            fixture.accept();
        }
        fixture
    }

    fn accept(&mut self) {
        let listener = self.listener.try_clone().unwrap();
        let (tx, rx) = mpsc::channel();
        let accept = std::thread::spawn(move || {
            let _ = tx.send(listener.accept());
        });
        let accepted = rx.recv_timeout(BOUND);
        if accepted.is_err() {
            let _ = TcpStream::connect(&self.address);
        }
        accept.join().unwrap();
        let (socket, _) = accepted.expect("fixture did not connect").unwrap();
        socket.set_read_timeout(Some(BOUND)).unwrap();
        socket.set_write_timeout(Some(BOUND)).unwrap();
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let (role, pid) = line.trim().split_once(':').unwrap();
        let exit = ExitReceipt::open(pid.parse().unwrap());
        self.peers.push(Peer {
            role: role.into(),
            socket: reader.into_inner(),
            exit,
        });
    }

    pub(super) fn parent_reaped(&mut self) {
        self.reaped.take().unwrap().recv_timeout(BOUND).unwrap();
    }

    pub(super) fn continue_parent(&mut self) {
        self.continue_parent.take().unwrap().send(()).unwrap();
    }

    pub(super) fn expire(&mut self) {
        self.expire.take().unwrap().send(()).unwrap();
    }

    pub(super) fn release(&mut self, role: &str) {
        self.peers
            .iter_mut()
            .find(|peer| peer.role == role)
            .unwrap()
            .socket
            .write_all(b"g")
            .unwrap();
    }

    pub(super) fn finish(mut self) -> Result<(), UpdateCommandFailure> {
        let result = self
            .result
            .recv_timeout(BOUND)
            .expect("executor result missing");
        self.cleanup();
        result
    }

    fn cleanup(&mut self) {
        if let Some(gate) = self.expire.take() {
            let _ = gate.send(());
        }
        if let Some(gate) = self.continue_parent.take() {
            let _ = gate.send(());
        }
        for peer in &mut self.peers {
            let _ = peer.socket.write_all(b"g");
        }
        for peer in self.peers.drain(..) {
            peer.exit.wait();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[test]
fn provider_update_fixture_entry() {
    let Ok(mode) = std::env::var("COURIER_FIXTURE_MODE") else {
        return;
    };
    if mode == "receipt" {
        let destination = std::env::var("COURIER_FIXTURE_RECEIPT").unwrap();
        let receipt = serde_json::json!({
            "executable": std::env::current_exe().unwrap(),
            "args": std::env::args_os().skip(1).collect::<Vec<_>>().iter().map(|value| value.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "cwd": std::env::current_dir().unwrap(),
            "environment": std::env::vars().collect::<std::collections::BTreeMap<_, _>>(),
        });
        std::fs::write(destination, serde_json::to_vec(&receipt).unwrap()).unwrap();
        println!("fixture-output-token");
        std::process::exit(0);
    }
    let address = std::env::var("COURIER_FIXTURE_ADDRESS").unwrap();
    let role = if mode == "descendant" {
        "descendant"
    } else {
        "parent"
    };
    let mut gate = TcpStream::connect(&address).unwrap();
    gate.set_read_timeout(Some(BOUND)).unwrap();
    gate.set_write_timeout(Some(BOUND)).unwrap();
    writeln!(gate, "{role}:{}", std::process::id()).unwrap();
    if role == "descendant" || mode == "blocked" {
        let mut byte = [0];
        if gate.read_exact(&mut byte).is_err() {
            std::process::exit(97);
        }
        std::process::exit(0);
    }
    let mut child = Command::new(std::env::current_exe().unwrap());
    codeterm_util::process_util::no_window_std(&mut child);
    child
        .args([ENTRY, "--exact", "--nocapture"])
        .env_clear()
        .env("COURIER_FIXTURE_MODE", "descendant")
        .env("COURIER_FIXTURE_ADDRESS", &address);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        child.env("SystemRoot", root);
    }
    if mode == "closed" || mode == "windows-wait" {
        child.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = child.spawn().unwrap();
    let mut byte = [0];
    if gate.read_exact(&mut byte).is_err() {
        child.kill().unwrap();
        child.wait().unwrap();
        std::process::exit(98);
    }
    if mode == "success" || mode == "nonzero" {
        assert!(child.wait().unwrap().success());
    }
    if mode == "nonzero" {
        std::io::stdout().write_all(&vec![b'x'; 65536]).unwrap();
        eprintln!("fixture-error-tail");
        std::process::exit(23);
    }
    std::process::exit(0);
}

struct ExitReceipt {
    #[cfg(unix)]
    handle: std::os::fd::OwnedFd,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl ExitReceipt {
    fn open(pid: u32) -> Self {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::FromRawFd;
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
            assert!(fd >= 0, "pidfd_open: {}", std::io::Error::last_os_error());
            Self {
                handle: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
            }
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::FromRawFd;
            let fd = unsafe { libc::kqueue() };
            assert!(fd >= 0);
            let handle = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
            let event = libc::kevent {
                ident: pid as usize,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
                fflags: libc::NOTE_EXIT,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            assert_eq!(
                unsafe { libc::kevent(fd, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) },
                0
            );
            Self { handle }
        }
        #[cfg(windows)]
        {
            let handle =
                unsafe { windows_sys::Win32::System::Threading::OpenProcess(0x00100000, 0, pid) };
            assert!(!handle.is_null());
            Self { handle }
        }
    }

    fn wait(&self) {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let mut event = libc::pollfd {
                fd: self.handle.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(
                unsafe { libc::poll(&mut event, 1, BOUND.as_millis() as i32) },
                1
            );
            assert_ne!(event.revents & libc::POLLIN, 0);
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            let timeout = libc::timespec {
                tv_sec: BOUND.as_secs() as _,
                tv_nsec: 0,
            };
            assert_eq!(
                unsafe {
                    libc::kevent(
                        self.handle.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        &mut event,
                        1,
                        &timeout,
                    )
                },
                1
            );
            assert_ne!(event.fflags & libc::NOTE_EXIT, 0);
            assert_eq!(event.flags & libc::EV_ERROR, 0);
        }
        #[cfg(windows)]
        assert_eq!(
            unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(
                    self.handle,
                    BOUND.as_millis() as u32,
                )
            },
            0
        );
    }
}

#[cfg(windows)]
impl Drop for ExitReceipt {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
