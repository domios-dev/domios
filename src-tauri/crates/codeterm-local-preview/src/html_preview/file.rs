use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub fn checked_path(raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("Local preview path must be absolute".into());
    }
    if raw.starts_with(['\\', '\0'])
        || raw.starts_with("//")
        || raw.len() > 4096
        || (!cfg!(windows) && raw.contains('\\'))
    {
        return Err("Local preview path is not a local disk path".into());
    }
    for (index, part) in raw.split(['/', '\\']).enumerate() {
        if part.is_empty() {
            continue;
        }
        let drive = index == 0
            && part.len() == 2
            && part.as_bytes()[0].is_ascii_alphabetic()
            && part.ends_with(':');
        if part == "."
            || part == ".."
            || (!drive && part.contains(':'))
            || part.chars().any(|c| c.is_control())
            || part.ends_with([' ', '.'])
        {
            return Err("Local preview path contains a forbidden component".into());
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(*prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "0" | "1"
                        | "2"
                        | "3"
                        | "4"
                        | "5"
                        | "6"
                        | "7"
                        | "8"
                        | "9"
                        | "\u{00b9}"
                        | "\u{00b2}"
                        | "\u{00b3}"
                )
            })
        }) {
            return Err("Local preview device paths are forbidden".into());
        }
    }
    #[cfg(target_os = "macos")]
    for (alias, target) in [("/tmp", "/private/tmp"), ("/var", "/private/var")] {
        // macOS exposes these protected system directories through root-owned aliases.
        if let Ok(rest) = path.strip_prefix(alias) {
            return Ok(Path::new(target).join(rest));
        }
    }
    Ok(path.to_path_buf())
}

pub(crate) struct PreviewSource {
    pub path: PathBuf,
    pub size: u64,
    file: File,
    #[cfg(windows)]
    _parents: Vec<File>,
}

impl PreviewSource {
    pub fn open(
        raw: &str,
        guard: &impl Fn(&Path, Option<&Path>) -> Result<(), String>,
    ) -> Result<Self, String> {
        let path = checked_path(raw)?;
        guard(&path, None)?;
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("Unable to resolve preview file: {e}"))?;
        guard(&path, Some(&canonical))?;
        let mut source =
            secure_open(&path).map_err(|e| format!("Unable to open preview file: {e}"))?;
        let metadata = source.file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || multiple_links(&source.file, &metadata)? {
            return Err("Local preview source is not a regular single-link file".into());
        }
        source.path = canonical;
        source.size = metadata.len();
        Ok(source)
    }

    pub fn read(&mut self, limit: u64) -> Result<Vec<u8>, String> {
        if self.size > limit {
            return Err(format!("Local preview source exceeds {limit} bytes"));
        }
        let mut bytes = Vec::new();
        (&mut self.file)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > limit
            || self.file.metadata().map_err(|e| e.to_string())?.len() > limit
        {
            return Err(format!("Local preview source exceeds {limit} bytes"));
        }
        Ok(bytes)
    }
}

#[cfg(unix)]
fn secure_open(path: &Path) -> std::io::Result<PreviewSource> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let mut parent = File::open("/")?;
    let parts: Vec<_> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    for (index, name) in parts.iter().enumerate() {
        let name = CString::new(name.as_bytes()).map_err(|_| std::io::ErrorKind::InvalidInput)?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if index + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // Descriptor-relative traversal prevents ancestor replacement from redirecting a read.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    Ok(PreviewSource {
        path: path.into(),
        size: 0,
        file: parent,
    })
}

#[cfg(unix)]
fn multiple_links(_: &File, metadata: &std::fs::Metadata) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink() != 1)
}

#[cfg(windows)]
fn secure_open(path: &Path) -> std::io::Result<PreviewSource> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut current = PathBuf::new();
    let mut parents = Vec::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if !matches!(part, Component::Normal(_)) {
            continue;
        }
        // Retained handles deny deletion/rename of every ancestor while this file is read.
        let handle = File::options()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)?;
        if handle.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(std::io::Error::other("Reparse points are forbidden"));
        }
        parents.push(handle);
    }
    let file = parents.pop().ok_or(std::io::ErrorKind::InvalidInput)?;
    Ok(PreviewSource {
        path: path.into(),
        size: 0,
        file,
        _parents: parents,
    })
}

#[cfg(windows)]
fn multiple_links(file: &File, _: &std::fs::Metadata) -> Result<bool, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(info.nNumberOfLinks != 1)
}

#[cfg(test)]
mod tests;
