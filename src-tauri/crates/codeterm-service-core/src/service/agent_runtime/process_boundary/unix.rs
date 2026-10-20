use super::Boundary;
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Instant,
};

pub(super) fn terminate(boundary: &Boundary) -> Result<(), String> {
    if boundary
        .infrastructure
        .get(&boundary.pid)
        .is_none_or(|root| born(boundary.pid).ok() != Some(root.token()))
    {
        return Err("Owned process group leader identity changed".into());
    }
    if unsafe { libc::kill(-(boundary.pid as i32), libc::SIGKILL) } == 0
        || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error().to_string())
    }
}

#[cfg(target_os = "linux")]
pub(super) fn ids(boundary: &Boundary) -> Result<Vec<u32>, String> {
    if let Some(root) = boundary.infrastructure.get(&boundary.pid) {
        if born(boundary.pid).ok() != Some(root.token()) {
            return Err("Owned process group leader identity changed".into());
        }
    }
    let mut ids = Vec::new();
    for entry in std::fs::read_dir("/proc").map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        {
            if unsafe { libc::getpgid(pid as i32) } == boundary.pid as i32 {
                ids.push(pid);
            }
        }
    }
    Ok(ids)
}
#[cfg(target_os = "linux")]
fn born(pid: u32) -> Result<u128, String> {
    let stat =
        std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|error| error.to_string())?;
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .ok_or_else(|| "Missing process start time".to_string())?
        .parse::<u128>()
        .map_err(|error| error.to_string())
}
#[cfg(target_os = "macos")]
pub(super) fn ids(boundary: &Boundary) -> Result<Vec<u32>, String> {
    if let Some(root) = boundary.infrastructure.get(&boundary.pid) {
        if born(boundary.pid).ok() != Some(root.token()) {
            return Err("Owned process group leader identity changed".into());
        }
    }
    let mut ids = vec![0u32; 257];
    let capacity = std::mem::size_of_val(ids.as_slice()) as i32;
    let size = unsafe { libc::proc_listpids(2, boundary.pid, ids.as_mut_ptr().cast(), capacity) };
    if size < 0 || size >= capacity {
        return Err("Owned process group snapshot failed or exceeded its bound".into());
    }
    ids.truncate(size as usize / std::mem::size_of::<u32>());
    ids.retain(|pid| *pid != 0);
    Ok(ids)
}
#[cfg(target_os = "macos")]
fn born(pid: u32) -> Result<u128, String> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    if unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            1,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    } != size
    {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(u128::from(info.pbi_start_tvsec) * 1_000_000 + u128::from(info.pbi_start_tvusec))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn ids(_: &Boundary) -> Result<Vec<u32>, String> {
    Err("Runtime process groups are unsupported on this OS".into())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn born(_: u32) -> Result<u128, String> {
    Err("Runtime process identities are unsupported on this OS".into())
}

pub(super) struct Member {
    pid: u32,
    born: u128,
    event: OwnedFd,
    observed_exit: std::sync::atomic::AtomicBool,
}
impl Member {
    pub(super) fn open(boundary: &Boundary, pid: u32) -> Result<Option<Self>, String> {
        let fd = event(pid);
        if fd < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(None);
            }
            return Err(error.to_string());
        }
        let event = unsafe { OwnedFd::from_raw_fd(fd) };
        let born = born(pid)?;
        #[cfg(target_os = "macos")]
        log::debug!(
            "[agent-runtime] retained identity captured pid={} born={} owned_group={}",
            pid,
            born,
            boundary.pid
        );
        let member = Self {
            pid,
            born,
            event,
            observed_exit: std::sync::atomic::AtomicBool::new(false),
        };
        member.validate_identity(boundary)?;
        Ok(Some(member))
    }
    pub(super) fn token(&self) -> u128 {
        self.born
    }
    pub(super) fn exited(&self) -> Result<bool, String> {
        if self
            .observed_exit
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(true);
        }
        let mut event = libc::pollfd {
            fd: self.event.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut event, 1, 0) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        if event.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err("Invalid retained process exit event".into());
        }
        if event.revents & libc::POLLIN == 0 {
            return Ok(false);
        }
        #[cfg(target_os = "macos")]
        {
            let mut note: libc::kevent = unsafe { std::mem::zeroed() };
            let timeout = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let count = unsafe {
                libc::kevent(
                    self.event.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &mut note,
                    1,
                    &timeout,
                )
            };
            if count != 1
                || note.ident != self.pid as libc::uintptr_t
                || note.filter != libc::EVFILT_PROC
                || note.fflags & libc::NOTE_EXIT == 0
                || note.flags & libc::EV_ERROR != 0
            {
                return Err("Unconfirmed retained process exit event".into());
            }
        }
        self.observed_exit
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(true)
    }
    pub(super) fn validate_identity(&self, boundary: &Boundary) -> Result<(), String> {
        if self.exited()? {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let current_born = born(self.pid);
            let current_group = unsafe { libc::getpgid(self.pid as i32) };
            if current_born
                .as_ref()
                .is_ok_and(|current_born| *current_born == self.born)
                && current_group == boundary.pid as i32
            {
                return Ok(());
            }

            let exit_confirmed = self.exited()?;
            log::debug!(
                "[agent-runtime] retained identity pid={} expected_born={} current_born={:?} expected_group={} current_group={} exit_confirmed={}",
                self.pid,
                self.born,
                current_born,
                boundary.pid,
                current_group,
                exit_confirmed
            );
            let classification = validate_identity_fields(
                current_born,
                current_group,
                self.born,
                boundary.pid as i32,
                exit_confirmed,
            )?;
            log::debug!(
                "[agent-runtime] retained identity pid={} classification={:?}",
                self.pid,
                classification
            );
            match classification {
                IdentityClassification::AwaitExit => {
                    // A failed group lookup cannot authorize a signal; only the retained exit event can resolve it.
                    self.wait(Instant::now() + std::time::Duration::from_secs(5))
                }
                IdentityClassification::Valid | IdentityClassification::Exited => Ok(()),
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            if born(self.pid)? != self.born
                || unsafe { libc::getpgid(self.pid as i32) } != boundary.pid as i32
            {
                return Err("Retained process identity or owned group changed".into());
            }
            Ok(())
        }
    }
    pub(super) fn terminate(&self, boundary: &Boundary) -> Result<(), String> {
        self.validate_identity(boundary)?;
        if self.exited()? {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.event.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        #[cfg(not(target_os = "linux"))]
        let result = unsafe { libc::kill(self.pid as i32, libc::SIGKILL) } as libc::c_long;
        if result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(io::Error::last_os_error().to_string())
        }
    }
    pub(super) fn wait(&self, deadline: Instant) -> Result<(), String> {
        if self.exited()? {
            return Ok(());
        }
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Owned process exit deadline".into());
            }
            let mut event = libc::pollfd {
                fd: self.event.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let millis = remaining
                .as_millis()
                .saturating_add(1)
                .min(i32::MAX as u128) as i32;
            let result = unsafe { libc::poll(&mut event, 1, millis) };
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if result <= 0
                || event.revents & libc::POLLIN == 0
                || event.revents & (libc::POLLERR | libc::POLLNVAL) != 0
            {
                return Err("Owned process exit event failed or reached its deadline".into());
            }
            return if self.exited()? {
                Ok(())
            } else {
                Err("Process exit event did not prove exit".into())
            };
        }
    }
}
#[cfg(target_os = "linux")]
fn event(pid: u32) -> i32 {
    unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) as i32 }
}
#[cfg(target_os = "macos")]
fn event(pid: u32) -> i32 {
    let fd = unsafe { libc::kqueue() };
    if fd < 0 {
        return fd;
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        unsafe { libc::close(fd) };
        return -1;
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
        unsafe { libc::close(fd) };
        -1
    } else {
        fd
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn event(_: u32) -> i32 {
    -1
}

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
enum IdentityClassification {
    Valid,
    AwaitExit,
    Exited,
}

#[cfg(target_os = "macos")]
fn validate_identity_fields(
    current_born: Result<u128, String>,
    current_group: i32,
    expected_born: u128,
    expected_group: i32,
    exit_confirmed: bool,
) -> Result<IdentityClassification, String> {
    if exit_confirmed {
        return Ok(IdentityClassification::Exited);
    }
    if current_born? != expected_born {
        return Err("Retained process identity or owned group changed".into());
    }
    if current_group == -1 {
        return Ok(IdentityClassification::AwaitExit);
    }
    if current_group != expected_group {
        return Err("Retained process identity or owned group changed".into());
    }
    Ok(IdentityClassification::Valid)
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
