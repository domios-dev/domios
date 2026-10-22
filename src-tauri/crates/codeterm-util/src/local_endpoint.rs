use std::io;

pub fn display_name(name: &str) -> String {
    if cfg!(windows) {
        format!(r"\\.\pipe\{name}")
    } else if cfg!(any(target_os = "linux", target_os = "android")) {
        format!("@{name}")
    } else {
        format!("/tmp/{name}")
    }
}

pub fn bind_error(target: impl std::fmt::Display, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("Failed to bind {target}: {error}"))
}

#[cfg(any(unix, test))]
fn stale_socket(owned_socket: bool, connect: Result<(), io::ErrorKind>) -> bool {
    owned_socket
        && matches!(
            connect,
            Err(io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound)
        )
}

pub fn prepare_namespaced(name: &str) -> io::Result<()> {
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    return prepare_path(std::path::Path::new("/tmp").join(name).as_path());
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    {
        let _ = name;
        Ok(())
    }
}

#[cfg(unix)]
pub fn prepare_path(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(bind_error(path.display(), error)),
    };
    let owned_socket =
        metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() };
    if !owned_socket {
        return Err(bind_error(
            path.display(),
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "existing endpoint is not a socket owned by this user",
            ),
        ));
    }
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    socket.set_nonblocking(true)?;
    let result = socket.connect(&socket2::SockAddr::unix(path)?);
    if !stale_socket(
        owned_socket,
        result.as_ref().map(|_| ()).map_err(io::Error::kind),
    ) {
        return Err(bind_error(
            path.display(),
            match result {
                Ok(()) => io::Error::new(io::ErrorKind::AddrInUse, "endpoint has a live listener"),
                Err(error) => error,
            },
        ));
    }
    let current = match std::fs::symlink_metadata(path) {
        Ok(current) => current,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(bind_error(path.display(), error)),
    };
    if !current.file_type().is_socket()
        || current.uid() != metadata.uid()
        || current.dev() != metadata.dev()
        || current.ino() != metadata.ino()
    {
        return Err(bind_error(
            path.display(),
            io::Error::other("endpoint changed during stale check"),
        ));
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(bind_error(path.display(), error)),
    }
}

#[cfg(test)]
mod tests;
