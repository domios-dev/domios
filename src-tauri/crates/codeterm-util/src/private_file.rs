use std::fs;
use std::io;
use std::path::Path;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_lock;
#[cfg(windows)]
mod windows_policy;

#[cfg(all(windows, any(test, feature = "test-support")))]
pub use windows::add_deny_ace_for_test;

pub(crate) fn open_lock(path: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        restrict(path)?;
        Ok(file)
    }
    #[cfg(windows)]
    {
        windows_lock::open(path)
    }
}

pub fn restrict(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(windows)]
    {
        windows_policy::tighten(path)
    }
}

pub fn restrict_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        windows_policy::tighten_directory(path)
    }
}

pub fn owner_only(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path)?;
        let expected = if metadata.is_dir() { 0o700 } else { 0o600 };
        Ok(metadata.permissions().mode() & 0o777 == expected)
    }
    #[cfg(windows)]
    {
        windows::mode_ok(path)
    }
}

pub fn create_dir_all(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty() || path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_dir_all(parent)?;
    }
    #[cfg(unix)]
    let created = {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    };
    #[cfg(windows)]
    let created = fs::create_dir(path);
    match created {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn restrict_if_present(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => restrict(path),
        Ok(_) => Err(io::Error::other("sensitive state must be a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::atomic_write::write_private_atomic(path, bytes)
}

pub(crate) fn prepare_replacement(target: &Path, temporary: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = target;
        restrict(temporary)
    }
    #[cfg(windows)]
    match fs::symlink_metadata(target) {
        Ok(_) => windows_policy::copy_tightened(target, temporary),
        Err(error) if error.kind() == io::ErrorKind::NotFound => windows::restrict(temporary),
        Err(error) => Err(error),
    }
}

pub(crate) fn preserve_permissions(target: &Path, temporary: &Path) -> io::Result<()> {
    let metadata = match fs::metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    #[cfg(windows)]
    windows_policy::copy_permissions(target, temporary)?;
    fs::set_permissions(temporary, metadata.permissions())
}

#[cfg(test)]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests;
