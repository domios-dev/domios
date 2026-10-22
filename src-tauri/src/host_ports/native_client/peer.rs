use super::*;

pub(super) fn validate_challenge(
    challenge: &NativeHostChallenge,
    ready: &ServerReady,
) -> Result<(), String> {
    if challenge.protocol != NATIVE_HOST_PROTOCOL
        || uuid::Uuid::parse_str(&challenge.challenge).is_err()
        || challenge.connection.instance_id != ready.instance_id
        || challenge.connection.identity != ready.identity
        || challenge.connection.server_epoch != ready.epoch
        || challenge.connection.generation == 0
    {
        return Err("Native-host challenge is stale or belongs to another server".into());
    }
    Ok(())
}

pub(super) fn verify_native_peer(
    stream: &Stream,
    ready: &ServerReady,
) -> io::Result<PinnedProcess> {
    #[cfg(unix)]
    let credentials = stream.peer_creds()?;
    #[cfg(unix)]
    if credentials.euid() != Some(unsafe { native_effective_uid() }) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native-host socket belongs to another user",
        ));
    }
    let pid = native_peer_pid(stream)?;
    let process = PinnedProcess::open(pid)?;
    if process.provenance()? != ready.process {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native-host socket peer differs from the authenticated server process",
        ));
    }
    process.ensure_alive()?;
    Ok(process)
}

#[cfg(not(target_os = "macos"))]
fn native_peer_pid(stream: &Stream) -> io::Result<u32> {
    stream
        .peer_creds()?
        .pid()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::other("Native-host peer PID is unavailable"))
}

#[cfg(target_os = "macos")]
fn native_peer_pid(stream: &Stream) -> io::Result<u32> {
    use std::os::fd::{AsFd, AsRawFd};
    let Stream::UdSocket(socket) = stream;
    let mut pid = 0i32;
    let mut size = std::mem::size_of_val(&pid) as u32;
    let result = unsafe {
        getsockopt(
            socket.as_fd().as_raw_fd(),
            0,
            2,
            (&mut pid as *mut i32).cast(),
            &mut size,
        )
    };
    if result != 0 || pid <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid as u32)
}

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn getsockopt(
        socket: i32,
        level: i32,
        option: i32,
        value: *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
}

#[cfg(unix)]
unsafe fn native_effective_uid() -> u32 {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn geteuid() -> u32;
        }
        geteuid()
    }
    #[cfg(not(target_os = "macos"))]
    {
        extern "C" {
            fn geteuid() -> u32;
        }
        geteuid()
    }
}
