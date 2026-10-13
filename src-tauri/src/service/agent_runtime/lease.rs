use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub(crate) struct Lease {
    _writer: OwnedFd,
    reader: OwnedFd,
}

impl Lease {
    pub(crate) fn configure(command: &mut tokio::process::Command) -> io::Result<Self> {
        let mut pipe = [0; 2];
        #[cfg(target_os = "linux")]
        let opened = unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) };
        #[cfg(not(target_os = "linux"))]
        let opened = unsafe { libc::pipe(pipe.as_mut_ptr()) };
        if opened != 0 {
            return Err(io::Error::last_os_error());
        }
        let original_reader = unsafe { OwnedFd::from_raw_fd(pipe[0]) };
        let original_writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
        fn duplicate(fd: &OwnedFd) -> io::Result<OwnedFd> {
            let duplicated = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
            if duplicated < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
        }
        let lease = Self {
            reader: duplicate(&original_reader)?,
            _writer: duplicate(&original_writer)?,
        };
        drop(original_reader);
        drop(original_writer);
        let reader = lease.reader.as_raw_fd();
        let max_fd = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        if max_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let owner = std::process::id() as libc::pid_t;
        command.process_group(0);
        unsafe {
            command.pre_exec(move || {
                let group = libc::getpid();
                let guardian = libc::fork();
                if guardian < 0 {
                    return Err(io::Error::last_os_error());
                }
                if guardian == 0 {
                    // The exec error pipe must close here or the daemon's spawn never returns.
                    for fd in 0..max_fd as libc::c_int {
                        if fd != reader {
                            libc::close(fd);
                        }
                    }
                    #[cfg(target_os = "macos")]
                    {
                        wait_for_owner_exit(owner, reader);
                        libc::kill(-group, libc::SIGKILL);
                        libc::_exit(0);
                    }
                    #[cfg(not(target_os = "macos"))]
                    let mut byte = 0u8;
                    #[cfg(not(target_os = "macos"))]
                    loop {
                        let count = libc::read(reader, (&mut byte as *mut u8).cast(), 1);
                        let interrupted = count < 0
                            && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR);
                        if super::ownership::lease_read(count, interrupted)
                            == super::ownership::LeaseRead::RetireGroup
                        {
                            libc::kill(-group, libc::SIGKILL);
                            libc::_exit(0);
                        }
                    }
                }
                #[cfg(target_os = "linux")]
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != owner {
                    return Err(io::Error::from_raw_os_error(libc::ECHILD));
                }
                Ok(())
            });
        }
        Ok(lease)
    }
}

#[cfg(target_os = "macos")]
unsafe fn wait_for_owner_exit(owner: libc::pid_t, reader: libc::c_int) {
    let queue = libc::kqueue();
    if queue < 0 {
        return;
    }
    let events = [
        libc::kevent {
            ident: owner as _,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        },
        libc::kevent {
            ident: reader as _,
            filter: libc::EVFILT_READ,
            flags: libc::EV_ADD,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        },
    ];
    if libc::kevent(
        queue,
        events.as_ptr(),
        2,
        std::ptr::null_mut(),
        0,
        std::ptr::null(),
    ) < 0
    {
        return;
    }
    let mut event: libc::kevent = std::mem::zeroed();
    loop {
        if libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null()) >= 0 {
            if event.filter != libc::EVFILT_READ {
                return;
            }
            let mut byte = 0u8;
            let count = libc::read(reader, (&mut byte as *mut u8).cast(), 1);
            let interrupted =
                count < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR);
            if super::ownership::lease_read(count, interrupted)
                == super::ownership::LeaseRead::RetireGroup
            {
                return;
            }
            continue;
        }
        if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn closing_owner_lease_kills_the_sidecar_group() {
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args(["-c", "printf R; read ignored"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        let lease = Lease::configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        // Child::wait closes its own stdin; keep this fixture blocked until the lease kills it.
        let stdin = child.stdin.take().unwrap();
        let mut ready = [0u8; 1];
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            child.stdout.as_mut().unwrap().read_exact(&mut ready),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(ready, [b'R']);
        drop(lease);
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.signal(), Some(libc::SIGKILL));
        drop(stdin);
    }
}
