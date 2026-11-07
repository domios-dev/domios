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
    let valid = metadata.is_file() && metadata.len() <= max_bytes;
    #[cfg(unix)]
    let valid = valid && unix_owned(&metadata, unsafe { libc::geteuid() });
    #[cfg(windows)]
    let valid = {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        valid
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && windows::owner_only(file)?
    };
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Expected an owner-only regular file",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn unix_owned(metadata: &std::fs::Metadata, uid: libc::uid_t) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == uid && metadata.mode() & 0o7777 == 0o600
}

#[cfg(test)]
mod tests;
