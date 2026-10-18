use std::{io, path::PathBuf, sync::mpsc::SyncSender};

pub struct WorkerSpawn {
    pub pid: u32,
    pub executable: PathBuf,
    pub witness: io::Result<ChildWitness>,
}

#[cfg(windows)]
pub type ChildWitness = std::os::windows::io::OwnedHandle;
#[cfg(unix)]
pub type ChildWitness = std::os::fd::OwnedFd;

pub(super) fn publish(
    observer: &SyncSender<WorkerSpawn>,
    child: &tokio::process::Child,
    path: &std::path::Path,
) {
    let Some(pid) = child.id() else { return };
    let _ = observer.try_send(WorkerSpawn {
        pid,
        executable: path.into(),
        witness: capture(child, pid),
    });
}

#[cfg(windows)]
fn capture(child: &tokio::process::Child, _: u32) -> io::Result<ChildWitness> {
    use std::os::windows::io::BorrowedHandle;
    let handle = child
        .raw_handle()
        .ok_or_else(|| io::Error::other("Child handle unavailable"))?;
    unsafe { BorrowedHandle::borrow_raw(handle).try_clone_to_owned() }
}

#[cfg(unix)]
fn capture(_: &tokio::process::Child, pid: u32) -> io::Result<ChildWitness> {
    use std::os::fd::FromRawFd;
    // The sole Child has never been polled or dropped: even an exited child is
    // unreaped here, so its PID cannot be reused while acquiring this witness.
    #[cfg(target_os = "linux")]
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) as i32 };
    #[cfg(target_os = "macos")]
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { ChildWitness::from_raw_fd(raw) };
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
        if unsafe { libc::kevent(raw, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(handle)
}

#[cfg(test)]
mod tests;
