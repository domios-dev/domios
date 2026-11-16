use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::io::unix::AsyncFd;

pub(super) struct ProcessIdentity {
    pid: u32,
    birth: (u64, u64),
}

impl ProcessIdentity {
    pub(super) fn capture(pid: u32) -> io::Result<Self> {
        Ok(Self {
            pid,
            birth: process_birth(pid)?,
        })
    }

    pub(super) fn kill_verified(&self, verified_pid: Option<u32>) -> io::Result<()> {
        if verified_pid != Some(self.pid) || process_birth(self.pid)? != self.birth {
            return Err(io::Error::other(
                "browser process identity changed; SIGKILL refused",
            ));
        }
        if unsafe { libc::kill(self.pid as i32, libc::SIGKILL) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

fn process_birth(pid: u32) -> io::Result<(u64, u64)> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    if unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    } != size
    {
        return Err(io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok((info.pbi_start_tvsec, info.pbi_start_tvusec))
}

fn queue() -> io::Result<OwnedFd> {
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

fn subscribe(queue: &OwnedFd, ident: usize, filter: i16, fflags: u32) -> io::Result<()> {
    let event = libc::kevent {
        ident,
        filter,
        flags: libc::EV_ADD | libc::EV_CLEAR,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn process_exit(pid: u32) -> io::Result<OwnedFd> {
    let queue = queue()?;
    subscribe(&queue, pid as usize, libc::EVFILT_PROC, libc::NOTE_EXIT)?;
    Ok(queue)
}

fn next_event(queue: &OwnedFd, timeout: Duration) -> io::Result<Option<libc::kevent>> {
    let timeout = libc::timespec {
        tv_sec: timeout.as_secs().min(i64::MAX as u64) as _,
        tv_nsec: timeout.subsec_nanos() as _,
    };
    let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
    let count = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            std::ptr::null(),
            0,
            event.as_mut_ptr(),
            1,
            &timeout,
        )
    };
    if count < 0 {
        Err(io::Error::last_os_error())
    } else if count == 0 {
        Ok(None)
    } else {
        let event = unsafe { event.assume_init() };
        if event.flags & libc::EV_ERROR != 0 {
            Err(io::Error::from_raw_os_error(event.data as i32))
        } else {
            Ok(Some(event))
        }
    }
}

pub(super) struct ExitEvents(AsyncFd<OwnedFd>);

impl ExitEvents {
    pub(super) fn register(queue: OwnedFd) -> io::Result<Self> {
        // The descriptor remains owned and unchanged until Tokio unregisters it.
        unsafe { AsyncFd::register_with_interest(queue, tokio::io::Interest::READABLE) }
            .map(Self)
            .map_err(io::Error::from)
    }

    pub(super) async fn exited(&self, pid: u32) -> io::Result<()> {
        loop {
            let mut ready = self.0.readable().await?;
            match ready.try_io(|queue| {
                next_event(queue.get_ref(), Duration::ZERO)?.ok_or(io::ErrorKind::WouldBlock.into())
            }) {
                Ok(Ok(event))
                    if event.filter == libc::EVFILT_PROC
                        && event.ident == pid as usize
                        && event.fflags & libc::NOTE_EXIT != 0 =>
                {
                    return Ok(())
                }
                Ok(Ok(_)) | Err(_) => continue,
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(error)) => return Err(error),
            }
        }
    }
}

pub(super) struct PortFile {
    queue: OwnedFd,
    _directory: File,
    path: PathBuf,
    previous: Option<String>,
}

pub(super) struct CancelPort(OwnedFd);

impl Drop for CancelPort {
    fn drop(&mut self) {
        let _ = subscribe(&self.0, 1, libc::EVFILT_USER, libc::NOTE_TRIGGER);
    }
}

impl PortFile {
    pub(super) fn watch(profile: &Path) -> io::Result<Self> {
        let queue = queue()?;
        let directory = File::open(profile)?;
        subscribe(&queue, 1, libc::EVFILT_USER, 0)?;
        subscribe(
            &queue,
            directory.as_raw_fd() as usize,
            libc::EVFILT_VNODE,
            libc::NOTE_WRITE | libc::NOTE_RENAME | libc::NOTE_DELETE,
        )?;
        let path = profile.join("DevToolsActivePort");
        let previous = read_port_file(&path)?.map(|(_, text)| text);
        Ok(Self {
            queue,
            _directory: directory,
            path,
            previous,
        })
    }

    pub(super) fn cancellation(&self) -> io::Result<CancelPort> {
        self.queue.try_clone().map(CancelPort)
    }

    pub(super) fn endpoint(self, deadline: Instant) -> Result<String, String> {
        loop {
            let watched_file = if let Some((file, _)) =
                read_port_file(&self.path).map_err(|error| error.to_string())?
            {
                subscribe(
                    &self.queue,
                    file.as_raw_fd() as usize,
                    libc::EVFILT_VNODE,
                    libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_RENAME | libc::NOTE_DELETE,
                )
                .map_err(|error| error.to_string())?;
                if let Some((_, text)) =
                    read_port_file(&self.path).map_err(|error| error.to_string())?
                {
                    if self.previous.as_ref() != Some(&text) {
                        if let Some(endpoint) = crate::launch::active_port_endpoint(&text) {
                            return Ok(endpoint);
                        }
                    }
                }
                Some(file)
            } else {
                None
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("browser DevToolsActivePort readiness deadline".into());
            }
            match next_event(&self.queue, remaining) {
                Ok(Some(event)) if event.filter == libc::EVFILT_USER => {
                    return Err("browser profile watch cancelled".into())
                }
                Ok(Some(_)) => {}
                Ok(None) => return Err("browser DevToolsActivePort readiness deadline".into()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(format!("browser profile watch failed: {error}")),
            }
            drop(watched_file);
        }
    }
}

fn read_port_file(path: &Path) -> io::Result<Option<(File, String)>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(
            "DevToolsActivePort must be a regular file",
        ));
    }
    let mut text = String::new();
    (&file).take(64 * 1024).read_to_string(&mut text)?;
    Ok(Some((file, text)))
}
