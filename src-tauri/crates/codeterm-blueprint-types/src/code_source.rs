#[cfg(unix)]
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::{Component, Path};

#[cfg(unix)]
pub fn read(directory: &Path, relative: &str) -> Result<String, &'static str> {
    let parts = Path::new(relative)
        .components()
        .map(|component| match component {
            Component::Normal(name) => {
                CString::new(name.as_encoded_bytes()).map_err(|_| "contract_violation")
            }
            _ => Err("contract_violation"),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err("contract_violation");
    }
    let root = directory.canonicalize().map_err(|_| "contract_violation")?;
    let root =
        CString::new(root.as_os_str().as_encoded_bytes()).map_err(|_| "contract_violation")?;
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("contract_violation");
    }
    let mut parent = unsafe { File::from_raw_fd(fd) };
    let mut source = None;
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if last { 0 } else { libc::O_DIRECTORY };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), part.as_ptr(), flags) };
        if fd < 0 {
            return Err("contract_violation");
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if last {
            source = Some(file);
            break;
        }
        parent = file;
    }
    let file = source.ok_or("contract_violation")?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err("contract_violation");
    }
    let mut text = String::new();
    file.take(16385)
        .read_to_string(&mut text)
        .map_err(|_| "contract_violation")?;
    if text.len() > 16384 {
        return Err("budget_exhausted");
    }
    Ok(text)
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::read;
