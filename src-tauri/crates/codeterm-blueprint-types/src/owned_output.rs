use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub struct OwnedOutput {
    root: File,
    ancestor: Option<(File, PathBuf)>,
    directory: PathBuf,
    temporary: String,
    name: String,
    file: Option<File>,
    published: bool,
    retained: bool,
}
impl OwnedOutput {
    pub fn create(directory: &Path, name: &str) -> io::Result<Self> {
        if name.is_empty()
            || name.len() > 240
            || name.contains(['/', '\\', ':', '\0'])
            || name.starts_with('.')
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let root = open_output_directory(directory)?;
        let temporary = format!(".transfer-{}.partial", uuid::Uuid::new_v4().simple());
        let file = create_file(&root, directory, &temporary)?;
        Ok(Self {
            root,
            ancestor: None,
            directory: directory.to_path_buf(),
            temporary,
            name: name.into(),
            file: Some(file),
            published: false,
            retained: false,
        })
    }
    pub fn create_child(directory: &Path, child: &str, name: &str) -> io::Result<Self> {
        if child.is_empty()
            || child.starts_with('.')
            || child.contains(['/', '\\', ':', '\0'])
            || name.is_empty()
            || name.len() > 240
            || name.starts_with('.')
            || name.contains(['/', '\\', ':', '\0'])
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let ancestor = open_output_directory(directory)?;
        let path = directory.join(child);
        let root = open_child(&ancestor, directory, child)?;
        let temporary = format!(".transfer-{}.partial", uuid::Uuid::new_v4().simple());
        let file = create_file(&root, &path, &temporary)?;
        Ok(Self {
            root,
            ancestor: Some((ancestor, directory.to_path_buf())),
            directory: path,
            temporary,
            name: name.into(),
            file: Some(file),
            published: false,
            retained: false,
        })
    }
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or(io::ErrorKind::BrokenPipe)?
            .write_all(bytes)
    }
    pub fn publish(&mut self) -> io::Result<PathBuf> {
        self.file
            .as_ref()
            .ok_or(io::ErrorKind::BrokenPipe)?
            .sync_all()?;
        drop(self.file.take());
        if let Some((root, path)) = &self.ancestor {
            same_root(root, path)?;
        }
        same_root(&self.root, &self.directory)?;
        publish(&self.root, &self.directory, &self.temporary, &self.name)?;
        self.published = true;
        #[cfg(unix)]
        self.root.sync_all()?;
        Ok(self.directory.join(&self.name))
    }
    pub fn retain(mut self) {
        self.retained = true;
    }
}
impl Drop for OwnedOutput {
    fn drop(&mut self) {
        drop(self.file.take());
        remove(&self.root, &self.directory, &self.temporary);
        if self.published && !self.retained {
            remove(&self.root, &self.directory, &self.name);
        }
    }
}
#[cfg(unix)]
fn open_output_directory(path: &Path) -> io::Result<File> {
    crate::code_source::open_directory(path)
        .map_err(|_| io::ErrorKind::PermissionDenied.into())
}
#[cfg(windows)]
fn open_output_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}
#[cfg(unix)]
fn open_child(parent: &File, _: &Path, name: &str) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name =
        std::ffi::CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
    }
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
#[cfg(windows)]
fn open_child(_: &File, path: &Path, name: &str) -> io::Result<File> {
    let path = path.join(name);
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    open_output_directory(&path)
}
#[cfg(unix)]
fn create_file(root: &File, _: &Path, name: &str) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name =
        std::ffi::CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
#[cfg(windows)]
fn create_file(_: &File, directory: &Path, name: &str) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    };
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(directory.join(name))
}
#[cfg(unix)]
fn same_root(root: &File, path: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let live = crate::code_source::open_directory(path)
        .map_err(|_| io::Error::from(io::ErrorKind::PermissionDenied))?;
    let (old, live) = (root.metadata()?, live.metadata()?);
    if old.dev() != live.dev() || old.ino() != live.ino() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
#[cfg(windows)]
fn same_root(_: &File, path: &Path) -> io::Result<()> {
    crate::code_source::open_directory(path)
        .map(|_| ())
        .map_err(|_| io::ErrorKind::PermissionDenied.into())
}
#[cfg(unix)]
fn publish(root: &File, _: &Path, from: &str, to: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let (from, to) = (
        std::ffi::CString::new(from).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
        std::ffi::CString::new(to).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
    );
    if unsafe {
        libc::linkat(
            root.as_raw_fd(),
            from.as_ptr(),
            root.as_raw_fd(),
            to.as_ptr(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
fn publish(_: &File, directory: &Path, from: &str, to: &str) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let wide = |name: &str| {
        directory
            .join(name)
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let (from, to) = (wide(from), wide(to));
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(unix)]
fn remove(root: &File, _: &Path, name: &str) {
    use std::os::fd::AsRawFd;
    if let Ok(name) = std::ffi::CString::new(name) {
        unsafe {
            libc::unlinkat(root.as_raw_fd(), name.as_ptr(), 0);
        }
    }
}
#[cfg(windows)]
fn remove(_: &File, directory: &Path, name: &str) {
    let _ = std::fs::remove_file(directory.join(name));
}
#[cfg(test)]
mod tests;
