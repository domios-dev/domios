use std::io;
use std::os::fd::{IntoRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::{Cancellation, Registration, Wake};

struct Descriptor {
    raw: Option<RawFd>,
    uncertain: bool,
}

impl Descriptor {
    fn new(raw: RawFd) -> Self {
        Self {
            raw: Some(raw),
            uncertain: false,
        }
    }

    fn close(&mut self) -> io::Result<()> {
        if self.uncertain {
            return Err(io::Error::other("probe event close remains unconfirmed"));
        }
        if let Some(fd) = self.raw.take() {
            if unsafe { libc::close(fd) } != 0 {
                self.uncertain = true;
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if let Some(fd) = self.raw.take() {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

struct SocketWake {
    writer: Mutex<Descriptor>,
    error: Mutex<Option<String>>,
}

impl Wake for SocketWake {
    fn wake(&self) {
        let writer = self.writer.lock();
        if let Some(fd) = writer.raw {
            #[cfg(target_os = "linux")]
            let flags = libc::MSG_NOSIGNAL;
            #[cfg(not(target_os = "linux"))]
            let flags = 0;
            let byte = [1u8];
            loop {
                if unsafe { libc::send(fd, byte.as_ptr().cast(), 1, flags) } == 1 {
                    break;
                }
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                *self.error.lock() = Some(error.to_string());
                break;
            }
        }
    }
}

pub struct Events {
    reader: Descriptor,
    wake: Arc<SocketWake>,
    registration: Option<Registration>,
    parent: Option<Descriptor>,
}

pub(in crate::probe) enum ParentEvent {
    Complete,
    Cancelled,
    Deadline,
}

impl Events {
    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::probe) fn waiting(&self, phase: super::WaitPhase) {
        if let Some(registration) = &self.registration {
            registration.waiting(phase);
        }
    }

    pub(in crate::probe) fn new(signal: &Cancellation) -> Result<Self, String> {
        require_parent_events()?;
        let (reader, writer) = UnixStream::pair().map_err(|error| error.to_string())?;
        reader
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        writer
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let wake = Arc::new(SocketWake {
            writer: Mutex::new(Descriptor::new(writer.into_raw_fd())),
            error: Mutex::new(None),
        });
        let reader = Descriptor::new(reader.into_raw_fd());
        #[cfg(target_os = "macos")]
        {
            let enabled: libc::c_int = 1;
            let fd = wake
                .writer
                .lock()
                .raw
                .ok_or("probe cancel writer missing")?;
            if unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_NOSIGPIPE,
                    (&enabled as *const libc::c_int).cast(),
                    std::mem::size_of_val(&enabled) as _,
                )
            } != 0
            {
                return Err(io::Error::last_os_error().to_string());
            }
        }
        let registration = Some(signal.register(wake.clone()).map_err(str::to_string)?);
        Ok(Self {
            reader,
            wake,
            registration,
            parent: None,
        })
    }

    pub(in crate::probe) fn cancel_fd(&self) -> RawFd {
        self.reader.raw.unwrap_or(-1)
    }

    pub(in crate::probe) fn observe_parent(
        &mut self,
        child: &std::process::Child,
    ) -> Result<(), String> {
        self.parent = Some(Descriptor::new(
            parent_event(child.id()).map_err(|error| error.to_string())?,
        ));
        Ok(())
    }

    pub(in crate::probe) fn wait_parent(
        &self,
        timeout: Duration,
        observe_cancel: bool,
    ) -> Result<ParentEvent, String> {
        #[cfg(any(test, feature = "test-support"))]
        self.waiting(super::WaitPhase::Parent);
        let parent = self
            .parent
            .as_ref()
            .and_then(|fd| fd.raw)
            .ok_or("probe parent completion event missing")?;
        let mut fds = [
            libc::pollfd {
                fd: parent,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if observe_cancel { self.cancel_fd() } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let end = std::time::Instant::now() + timeout;
        loop {
            let remaining = end.saturating_duration_since(std::time::Instant::now());
            let millis = remaining
                .as_millis()
                .saturating_add(u128::from(remaining.subsec_nanos() % 1_000_000 != 0))
                .min(i32::MAX as u128) as i32;
            let result = unsafe { libc::poll(fds.as_mut_ptr(), 2, millis) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.to_string());
            }
            if result == 0 {
                return Ok(ParentEvent::Deadline);
            }
            if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
                return Err("probe owned event became invalid".into());
            }
            if fds[0].revents != 0 {
                return Ok(ParentEvent::Complete);
            }
            if fds[1].revents != 0 {
                return Ok(ParentEvent::Cancelled);
            }
        }
    }

    pub(in crate::probe) fn take_delivery_error(&self) -> Option<String> {
        self.wake.error.lock().take()
    }

    pub(in crate::probe) fn cancelled(&self) -> bool {
        self.registration.as_ref().is_some_and(|registration| {
            registration
                .signal
                .load(std::sync::atomic::Ordering::SeqCst)
        })
    }

    pub(in crate::probe) fn close(&mut self) -> Result<(), String> {
        self.registration.take();
        let mut errors = Vec::new();
        if let Err(error) = self.wake.writer.lock().close() {
            errors.push(error.to_string());
        }
        if let Err(error) = self.reader.close() {
            errors.push(error.to_string());
        }
        if let Some(parent) = &mut self.parent {
            if let Err(error) = parent.close() {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(target_os = "linux")]
pub(in crate::probe) fn parent_event(pid: u32) -> io::Result<RawFd> {
    // The descriptor observes exit without reaping or granting signal authority.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd as RawFd)
    }
}

#[cfg(target_os = "macos")]
pub(in crate::probe) fn parent_event(pid: u32) -> io::Result<RawFd> {
    let fd = unsafe { libc::kqueue() };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut owned = Descriptor::new(fd);
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let event = libc::kevent {
        ident: pid as _,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    if unsafe { libc::kevent(fd, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    owned.raw.take();
    Ok(fd)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(in crate::probe) fn parent_event(_: u32) -> io::Result<RawFd> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "probe_cancel_parent_wait_unsupported_platform",
    ))
}

#[cfg(target_os = "linux")]
fn require_parent_events() -> Result<(), String> {
    let result = unsafe { libc::syscall(libc::SYS_pidfd_open, -1i32, 0) };
    let error = io::Error::last_os_error();
    if result == -1 && error.raw_os_error() == Some(libc::EINVAL) {
        Ok(())
    } else {
        Err(format!(
            "probe_cancel_parent_wait_unsupported_kernel: {error}"
        ))
    }
}

#[cfg(target_os = "macos")]
fn require_parent_events() -> Result<(), String> {
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn require_parent_events() -> Result<(), String> {
    Err("probe_cancel_parent_wait_unsupported_platform".into())
}
