use codeterm_types::server_bootstrap::ProcessProvenance;
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
};

pub struct PinnedProcess {
    handle: OwnedFd,
    observed: ProcessProvenance,
}

impl PinnedProcess {
    pub fn from_child_witness(pid: u32, handle: OwnedFd) -> io::Result<Self> {
        let mut fd = libc::pollfd {
            fd: handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 0) } != 0 {
            return Err(io::Error::other(
                "Observed maintenance child already exited",
            ));
        }
        let observed = observe(pid)?;
        let result = Self { handle, observed };
        result.ensure_alive()?;
        Ok(result)
    }

    pub fn open(pid: u32) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) as i32 };
        #[cfg(target_os = "macos")]
        let raw = unsafe { libc::kqueue() };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let handle = unsafe { OwnedFd::from_raw_fd(raw) };
        if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        #[cfg(target_os = "macos")]
        {
            let event = libc::kevent {
                ident: pid as _,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD | libc::EV_ENABLE,
                fflags: libc::NOTE_EXIT,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            if unsafe { libc::kevent(raw, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) }
                < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let observed = observe(pid)?;
        let result = Self { handle, observed };
        result.ensure_alive()?;
        Ok(result)
    }

    pub fn wait_exit(&self, deadline: std::time::Duration) -> io::Result<()> {
        let mut fd = libc::pollfd {
            fd: self.handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut fd,
                1,
                deadline.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if result == 1 && fd.revents & libc::POLLIN != 0 {
            Ok(())
        } else {
            Err(io::Error::other("Exact process exit deadline"))
        }
    }

    pub fn ensure_alive(&self) -> io::Result<()> {
        let mut fd = libc::pollfd {
            fd: self.handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 0) } != 0 {
            return Err(io::Error::other("Bootstrap peer exited"));
        }
        Ok(())
    }

    pub fn provenance(&self) -> io::Result<ProcessProvenance> {
        self.ensure_alive()?;
        let current = observe(self.observed.pid)?;
        if current != self.observed {
            return Err(io::Error::other(format!(
                "Bootstrap PID provenance changed: pinned={:?}; current={:?}",
                self.observed, current
            )));
        }
        self.ensure_alive()?;
        Ok(current)
    }
}

#[cfg(target_os = "linux")]
fn observe(pid: u32) -> io::Result<ProcessProvenance> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("Invalid process stat"))?
        .1;
    let created = fields
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| io::Error::other("Missing process creation token"))?
        .to_string();
    Ok(ProcessProvenance {
        pid,
        created,
        executable: PathBuf::from(format!("/proc/{pid}/exe")).canonicalize()?,
    })
}

#[cfg(target_os = "macos")]
fn observe(pid: u32) -> io::Result<ProcessProvenance> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    if unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    } != size
    {
        return Err(io::Error::last_os_error());
    }
    let mut image = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count =
        unsafe { libc::proc_pidpath(pid as i32, image.as_mut_ptr().cast(), image.len() as u32) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    use std::os::unix::ffi::OsStrExt;
    let length = image
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(count as usize);
    let path = std::ffi::OsStr::from_bytes(&image[..length]);
    Ok(ProcessProvenance {
        pid,
        created: format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        executable: PathBuf::from(path).canonicalize()?,
    })
}
