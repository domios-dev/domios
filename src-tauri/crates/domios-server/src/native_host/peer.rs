use crate::bootstrap::process::PinnedProcess;
use interprocess::local_socket::tokio::{prelude::*, Stream};
use std::io;

pub(super) fn pin_peer(stream: &Stream) -> io::Result<PinnedProcess> {
    let credentials = stream.peer_creds()?;
    #[cfg(unix)]
    if credentials.euid() != Some(unsafe { libc::geteuid() }) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Foreign native host user",
        ));
    }
    #[cfg(not(target_os = "macos"))]
    let pid = credentials
        .pid()
        .ok_or_else(|| io::Error::other("Native host peer PID unavailable"))? as u32;
    #[cfg(target_os = "macos")]
    let pid = {
        use std::os::fd::{AsFd, AsRawFd};
        let Stream::UdSocket(socket) = stream;
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                socket.as_fd().as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        pid as u32
    };
    PinnedProcess::open(pid)
}
