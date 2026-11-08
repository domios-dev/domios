use std::fs::File;
use std::io;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS};

pub(super) fn open(path: &Path) -> io::Result<File> {
    let file = super::windows_create::open(path, OPEN_ALWAYS, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
    super::windows::restrict(path)?;
    Ok(file)
}
