use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(windows)]
mod windows;

pub fn create_new_owner_only(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?
    };
    #[cfg(windows)]
    let file = windows::create_new(path)?;
    validate(&file, 0)?;
    Ok(file)
}

pub fn open_owner_only(path: &Path, max_bytes: u64) -> io::Result<File> {
    let file = open_no_follow(path)?;
    validate(&file, max_bytes)?;
    Ok(file)
}

pub fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    #[cfg(windows)]
    super::windows_create::create_dir(path)?;
    open_directory_owner_only(path)?;
    Ok(())
}

pub fn open_directory_owner_only(path: &Path) -> io::Result<File> {
    let file = open_no_follow(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || !handle_owner_only(&file, &metadata, 0o700)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Expected an owner-only directory",
        ));
    }
    Ok(file)
}

fn open_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path)
}

fn validate(file: &File, max_bytes: u64) -> io::Result<()> {
    let metadata = file.metadata()?;
    let valid = metadata.is_file()
        && metadata.len() <= max_bytes
        && handle_owner_only(file, &metadata, 0o600)?;
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Expected an owner-only regular file",
        ));
    }
    Ok(())
}

fn handle_owner_only(file: &File, metadata: &std::fs::Metadata, _mode: u32) -> io::Result<bool> {
    #[cfg(unix)]
    {
        let _ = file;
        Ok(unix_owned(metadata, unsafe { libc::geteuid() }, _mode))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        Ok(
            metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
                && windows::owner_only(file)?,
        )
    }
}

#[cfg(unix)]
fn unix_owned(metadata: &std::fs::Metadata, uid: libc::uid_t, mode: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == uid && metadata.mode() & 0o7777 == mode
}

#[cfg(test)]
mod tests;
