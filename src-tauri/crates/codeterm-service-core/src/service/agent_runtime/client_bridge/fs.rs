use super::BridgeError;
#[cfg(not(windows))]
use std::path::{Component, PathBuf};
#[cfg(windows)]
pub(super) mod windows;
#[cfg(windows)]
pub(super) use windows::{directory, root_matches, write};

use std::{fs::File, io::Read, path::Path};

pub(super) const MAX_BYTES: usize = 1024 * 1024;
pub(super) const MAX_PATH: usize = 4096;

#[cfg(windows)]
pub(super) use windows::relative;

#[cfg(not(windows))]
pub(super) fn relative(root: &Path, raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty()
        || raw.len() > MAX_PATH
        || raw.chars().any(char::is_control)
        || raw.starts_with("//")
        || raw.starts_with("\\\\")
        || raw.contains('\\')
        || raw
            .split('/')
            .any(|part| part == ".." || part.contains(':'))
    {
        return Err("Invalid bridge path".into());
    }
    let path = Path::new(raw);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Bridge paths must be absolute without traversal".into());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Path outside workspace")?;
    if relative.as_os_str().is_empty() {
        return Err("Expected a workspace file".into());
    }
    Ok(relative.to_path_buf())
}

pub(super) fn machine_secret(path: &Path) -> bool {
    path.components().any(|part| {
        [
            ".ssh",
            ".aws",
            ".azure",
            ".kube",
            ".gnupg",
            ".domios",
            ".codeterm",
        ]
        .contains(
            &part
                .as_os_str()
                .to_string_lossy()
                .to_ascii_lowercase()
                .as_str(),
        )
    })
}

pub(super) fn secret(path: &Path) -> bool {
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        name.starts_with(".env")
            || name.ends_with(".pem")
            || name.starts_with("id_rsa")
            || name.starts_with("id_ed25519")
            || name.contains("credential")
            || name.contains("token")
            || name.contains("secret")
            || [".npmrc", ".netrc", ".pypirc", "auth.json"].contains(&name.as_str())
            || [
                ".ssh",
                ".aws",
                ".azure",
                ".kube",
                ".gnupg",
                ".domios",
                ".codeterm",
            ]
            .contains(&name.as_str())
    })
}

#[cfg(not(windows))]
pub(super) fn check(root: &Path, relative: &Path, missing_leaf: bool) -> Result<(), BridgeError> {
    let mut path = root.to_path_buf();
    let parts = relative.components().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        if !matches!(part, Component::Normal(_)) {
            return Err("Invalid path component".into());
        }
        path.push(part.as_os_str());
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("reparse_point: Symlink traversal denied".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err("Reparse traversal denied".into());
                    }
                }
                if index + 1 == parts.len() {
                    if !metadata.is_file() {
                        return Err("Non-regular file denied".into());
                    }
                } else if !metadata.is_dir() {
                    return Err("Non-directory component".into());
                }
            }
            Err(error)
                if missing_leaf
                    && index + 1 == parts.len()
                    && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(if index + 1 == parts.len() {
                    BridgeError::NotFound
                } else {
                    "Workspace parent unavailable".into()
                });
            }
            Err(error) => return Err(error.to_string().into()),
        }
    }
    Ok(())
}

pub(super) fn read(
    root: &Path,
    root_handle: &File,
    relative: &Path,
    line: Option<usize>,
    limit: Option<usize>,
) -> Result<String, BridgeError> {
    root_matches(root, root_handle)?;
    check(root, relative, false)?;
    #[cfg(unix)]
    let file = {
        let parent = directory(root_handle, relative.parent().ok_or("Missing parent")?)?;
        let name = std::ffi::CString::new(
            relative
                .file_name()
                .ok_or("Missing filename")?
                .as_encoded_bytes(),
        )
        .map_err(|error| error.to_string())?;
        let file = open_read_at_no_follow(&parent, &name, libc::O_RDONLY | libc::O_NONBLOCK)?;
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("Non-regular file denied".into());
        }
        file
    };
    #[cfg(not(any(unix, windows)))]
    let file = codeterm_blueprint_types::code_source::open_file(
        root,
        relative.to_str().ok_or("Invalid UTF-8 path")?,
    )
    .map_err(str::to_string)?;
    #[cfg(windows)]
    let file = windows::read(root_handle, relative)?;
    read_file(file, line, limit).map_err(BridgeError::from)
}

fn read_file(file: File, line: Option<usize>, limit: Option<usize>) -> Result<String, String> {
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_BYTES as u64 {
        return Err("Read exceeds 1 MiB".into());
    }
    let mut text = String::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() > MAX_BYTES {
        return Err("Read exceeds 1 MiB".into());
    }
    let start = line
        .unwrap_or(1)
        .checked_sub(1)
        .ok_or("Line is one-based")?;
    if line.is_none() && limit.is_none() {
        return Ok(text);
    }
    Ok(text
        .split_inclusive('\n')
        .skip(start)
        .take(limit.unwrap_or(usize::MAX))
        .collect())
}

#[cfg(unix)]
pub(super) fn write(
    root: &Path,
    root_handle: &File,
    relative: &Path,
    content: &str,
    commit: impl FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    write_with_sync(root, root_handle, relative, content, commit, |parent| {
        parent.sync_all().map_err(|error| error.to_string())
    })
}

#[cfg(unix)]
pub(super) fn write_with_sync(
    root: &Path,
    root_handle: &File,
    relative: &Path,
    content: &str,
    commit: impl FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
    mut sync_parent: impl FnMut(&File) -> Result<(), String>,
) -> Result<(), String> {
    use std::{ffi::CString, io::Write, os::fd::AsRawFd};
    if content.len() > MAX_BYTES {
        return Err("Write exceeds 1 MiB".into());
    }
    check(root, relative, true).map_err(|error| error.to_string())?;
    root_matches(root, root_handle)?;
    let parent_handle = directory(root_handle, relative.parent().ok_or("Missing parent")?)?;
    let name = CString::new(
        relative
            .file_name()
            .ok_or("Missing filename")?
            .as_encoded_bytes(),
    )
    .map_err(|error| error.to_string())?;
    let temporary = CString::new(format!(".domios-bridge-{}", uuid::Uuid::new_v4()))
        .map_err(|error| error.to_string())?;
    let mut file = open_at_no_follow(
        &parent_handle,
        &temporary,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    let mut renamed = false;
    let result = (|| {
        file.write_all(content.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        let mut apply = || {
            root_matches(root, root_handle)?;
            check(root, relative, true).map_err(|error| error.to_string())?;
            let current_parent =
                directory(root_handle, relative.parent().ok_or("Missing parent")?)?;
            use std::os::unix::fs::MetadataExt;
            let current = current_parent
                .metadata()
                .map_err(|error| error.to_string())?;
            let held = parent_handle
                .metadata()
                .map_err(|error| error.to_string())?;
            if current.dev() != held.dev() || current.ino() != held.ino() {
                return Err("Workspace parent changed".into());
            }
            if unsafe {
                libc::renameat(
                    parent_handle.as_raw_fd(),
                    temporary.as_ptr(),
                    parent_handle.as_raw_fd(),
                    name.as_ptr(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            renamed = true;
            sync_parent(&parent_handle).map_err(|error| {
                format!(
                    "write_committed_unsynced: File was written; directory sync failed: {error}"
                )
            })
        };
        commit(&mut apply)
    })();
    if !renamed {
        unsafe {
            libc::unlinkat(parent_handle.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result
}

#[cfg(not(any(unix, windows)))]
pub(super) fn write(
    _root: &Path,
    _root_handle: &File,
    _relative: &Path,
    _content: &str,
    _commit: impl FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    Err("Atomic no-follow writes are unavailable on this platform".into())
}

#[cfg(unix)]
pub(super) fn root_matches(root: &Path, held: &File) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let filesystem_root = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|error| error.to_string())?;
    let current = directory(
        &filesystem_root,
        root.strip_prefix("/")
            .map_err(|_| "Invalid workspace root")?,
    )?
    .metadata()
    .map_err(|error| error.to_string())?;
    let held = held.metadata().map_err(|error| error.to_string())?;
    if current.dev() != held.dev() || current.ino() != held.ino() {
        return Err("Workspace root replaced".into());
    }
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub(super) fn root_matches(_root: &Path, _held: &File) -> Result<(), String> {
    Err("Client bridge unavailable".into())
}

#[cfg(unix)]
pub(super) fn directory(root: &File, relative: &Path) -> Result<File, String> {
    use std::ffi::CString;
    let mut parent = root.try_clone().map_err(|error| error.to_string())?;
    for part in relative.components() {
        let Component::Normal(part) = part else {
            return Err("Invalid directory component".into());
        };
        let name = CString::new(part.as_encoded_bytes()).map_err(|error| error.to_string())?;
        parent = open_at_no_follow(&parent, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    }
    Ok(parent)
}
#[cfg(unix)]
pub(super) fn open_at_no_follow(
    parent: &File,
    name: &std::ffi::CStr,
    flags: i32,
) -> Result<File, String> {
    open_read_at_no_follow(parent, name, flags).map_err(|error| match error {
        BridgeError::NotFound => "Workspace component unavailable".into(),
        BridgeError::Refused(message) => message,
    })
}

#[cfg(unix)]
fn open_read_at_no_follow(
    parent: &File,
    name: &std::ffi::CStr,
    flags: i32,
) -> Result<File, BridgeError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // Some kernels return ENOTDIR for O_DIRECTORY | O_NOFOLLOW on a symlink.
        let symlink = error.raw_os_error() == Some(libc::ELOOP)
            || (error.raw_os_error() == Some(libc::ENOTDIR)
                && unsafe {
                    libc::fstatat(
                        parent.as_raw_fd(),
                        name.as_ptr(),
                        metadata.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } == 0
                && unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT == libc::S_IFLNK);
        return Err(if symlink {
            "reparse_point: Symlink traversal denied".into()
        } else if error.kind() == std::io::ErrorKind::NotFound {
            BridgeError::NotFound
        } else {
            error.to_string().into()
        });
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(any(unix, windows)))]
pub(super) fn directory(_root: &File, _relative: &Path) -> Result<File, String> {
    Err("Client bridge unavailable".into())
}

#[cfg(windows)]
pub(super) fn check(root: &Path, relative: &Path, missing_leaf: bool) -> Result<(), BridgeError> {
    windows::check(root, relative, missing_leaf)
}
