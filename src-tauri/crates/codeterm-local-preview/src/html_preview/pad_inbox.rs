use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use super::file::{checked_path, secure_open};

pub fn write_pad_inbox(pad: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let pad = checked_path(&pad.to_string_lossy())?;
    let inbox = pad.join("inbox");
    for (name, _) in files {
        if Path::new(name).file_name().and_then(|name| name.to_str()) != Some(name)
            || checked_path(&inbox.join(name).to_string_lossy()).is_err()
        {
            return Err("Invalid pad inbox filename".into());
        }
    }
    write(&pad, files).map_err(|error| format!("Unable to write pad inbox snapshot: {error}"))
}

fn write_batch(
    files: &[(String, Vec<u8>)],
    mut create: impl FnMut(&str) -> io::Result<File>,
    mut remove: impl FnMut(&str),
) -> io::Result<()> {
    let mut created = Vec::with_capacity(files.len());
    let result = (|| {
        for (name, bytes) in files {
            let mut file = create(name)?;
            created.push(name);
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        for name in created {
            remove(name);
        }
    }
    result
}

#[cfg(unix)]
fn write(pad: &Path, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    use std::ffi::{CStr, CString};
    use std::os::fd::{AsRawFd, FromRawFd};

    let pad_handle = secure_open(pad)?;
    let root = pad_handle.file.as_raw_fd();
    if unsafe { libc::mkdirat(root, c"inbox".as_ptr(), 0o700) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
    }
    let open = |parent, name: &CStr, flags| {
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    };
    let inbox = open(root, c"inbox", libc::O_RDONLY | libc::O_DIRECTORY)?;
    let fd = inbox.as_raw_fd();
    write_batch(
        files,
        |name| {
            open(
                fd,
                &CString::new(name)?,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            )
        },
        |name| {
            if let Ok(name) = CString::new(name) {
                unsafe { libc::unlinkat(fd, name.as_ptr(), 0) };
            }
        },
    )
}

#[cfg(windows)]
fn write(pad: &Path, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    let _root = secure_open(pad)?;
    let inbox = pad.join("inbox");
    if let Err(error) = std::fs::create_dir(&inbox) {
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
    }
    let _inbox = secure_open(&inbox)?;
    write_batch(
        files,
        |name| {
            File::options()
                .write(true)
                .create_new(true)
                .open(inbox.join(name))
        },
        |name| {
            let _ = std::fs::remove_file(inbox.join(name));
        },
    )
}
