use codeterm_process_identity::PinnedProcess;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub struct PidFile {
    temporary: tempfile::NamedTempFile,
    destination: PathBuf,
    recovery_lock: Option<File>,
}

impl PidFile {
    pub fn prepare(destination: &Path) -> io::Result<Self> {
        if !destination.is_absolute() {
            return Err(io::Error::other("PID file path must be absolute"));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("PID file requires a parent directory"))?;
        let recovery_lock = match destination.symlink_metadata() {
            Ok(metadata) if metadata.is_file() => Some(lock_stale_pid(destination)?),
            Ok(_) => return Err(io::Error::other("Expected a regular PID file")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            temporary: tempfile::NamedTempFile::new_in(parent)?,
            destination: destination.into(),
            recovery_lock,
        })
    }

    pub fn publish(mut self, pid: u32) -> io::Result<()> {
        if pid == 0 {
            return Err(io::Error::other("Server PID must be nonzero"));
        }
        writeln!(self.temporary, "{pid}")?;
        self.temporary.as_file().sync_all()?;
        if self.recovery_lock.is_some() {
            self.temporary.persist(self.destination)
        } else {
            self.temporary.persist_noclobber(self.destination)
        }
        .map_err(|error| error.error)?;
        Ok(())
    }
}

fn lock_stale_pid(destination: &Path) -> io::Result<File> {
    let mut lock_path = destination.as_os_str().to_owned();
    lock_path.push(".recovery.lock");
    let lock = regular_file_options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    if !lock.metadata()?.is_file() {
        return Err(io::Error::other("Expected a regular PID recovery lock"));
    }
    lock.try_lock()?;
    let metadata = destination.symlink_metadata()?;
    if !metadata.is_file() || metadata.len() > 12 {
        return Err(io::Error::other("Invalid PID file"));
    }
    let mut contents = String::new();
    let record = regular_file_options().read(true).open(destination)?;
    if !record.metadata()?.is_file() {
        return Err(io::Error::other("Expected a regular PID file"));
    }
    record.take(13).read_to_string(&mut contents)?;
    let pid = contents
        .trim_end_matches(['\r', '\n'])
        .parse::<std::num::NonZeroU32>()
        .map_err(io::Error::other)?;
    let process = match PinnedProcess::open(pid.get()).and_then(|p| p.provenance()) {
        Ok(process) => process,
        Err(error) if process_absent(&error) => return Ok(lock),
        Err(error) => return Err(error),
    };
    let executable = std::env::current_exe()?.canonicalize()?;
    if process.executable.file_name() == executable.file_name() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "PID file belongs to a live server",
        ));
    }
    Ok(lock)
}

fn regular_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

fn process_absent(error: &io::Error) -> bool {
    #[cfg(unix)]
    return error.raw_os_error() == Some(libc::ESRCH);
    #[cfg(windows)]
    return error
        .get_ref()
        .is_some_and(|cause| cause.is::<codeterm_process_identity::ProcessExited>());
}

#[cfg(test)]
mod tests;
