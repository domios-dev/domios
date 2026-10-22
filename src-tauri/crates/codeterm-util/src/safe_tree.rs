use std::fs;
use std::path::{Path, PathBuf};

mod holders;

pub fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

pub fn extended_path(path: &Path) -> std::io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let name: Vec<u16> = path.as_os_str().encode_wide().collect();
        let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
        if name.starts_with(&prefix) {
            return Ok(path);
        }
        let value = if name.starts_with(&[92, 92]) {
            r"\\?\UNC\"
                .encode_utf16()
                .chain(name[2..].iter().copied())
                .collect::<Vec<_>>()
        } else {
            prefix.into_iter().chain(name).collect()
        };
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&value)))
    }
    #[cfg(not(windows))]
    Ok(path)
}

pub fn refuse_linked_ancestors(path: &Path) -> Result<(), String> {
    let absolute = extended_path(path).map_err(|error| error.to_string())?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(format!(
                    "refusing path through reparse point or symlink '{}'",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(failure("inspect", &current, error)),
        }
    }
    Ok(())
}

fn failure(action: &str, path: &Path, error: std::io::Error) -> String {
    format!(
        "cannot {action} '{}': {error}; holder: {}",
        path.display(),
        holders::describe(path)
    )
}

pub fn remove(path: &Path) -> Result<(), String> {
    let absolute = extended_path(path).map_err(|error| failure("resolve", path, error))?;
    match fs::symlink_metadata(&absolute) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failure("inspect", path, error)),
        Ok(metadata) if is_link(&metadata) => {
            return Err(format!(
                "refusing reparse point or symlink root '{}'",
                path.display()
            ));
        }
        Ok(_) => {}
    }
    let root = fs::canonicalize(&absolute).map_err(|error| failure("resolve", path, error))?;
    let mut pending = vec![(root.clone(), false)];
    while let Some((path, visited)) = pending.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(failure("inspect", &path, error)),
        };
        if is_link(&metadata) {
            unlink(&path, &metadata).map_err(|error| failure("unlink", &path, error))?;
        } else if metadata.is_dir() && !visited {
            pending.push((path.clone(), true));
            for entry in fs::read_dir(&path).map_err(|error| failure("read", &path, error))? {
                let entry = entry.map_err(|error| failure("read entry in", &path, error))?;
                pending.push((entry.path(), false));
            }
        } else {
            #[cfg(windows)]
            if metadata.permissions().readonly() {
                let mut permissions = metadata.permissions();
                // Windows-only: clears the read-only attribute; the Unix world-writable hazard does not apply.
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                fs::set_permissions(&path, permissions)
                    .map_err(|error| failure("clear read-only", &path, error))?;
            }
            let result = if metadata.is_dir() {
                fs::remove_dir(&path)
            } else {
                fs::remove_file(&path)
            };
            result.map_err(|error| failure("remove", &path, error))?;
        }
    }
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failure("verify removal of", &root, error)),
        Ok(_) => Err(format!(
            "directory remains after removal: '{}'",
            root.display()
        )),
    }
}

fn unlink(path: &Path, metadata: &fs::Metadata) -> std::io::Result<()> {
    clear_readonly_link_attribute(path, metadata).map_err(std::io::Error::other)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x10 != 0 {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests;

#[cfg(windows)]
fn clear_readonly_link_attribute(path: &Path, metadata: &std::fs::Metadata) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileBasicInfo, SetFileInformationByHandle, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
    };

    let attributes = metadata.file_attributes();
    if attributes & FILE_ATTRIBUTE_READONLY == 0 {
        return Ok(());
    }
    let handle = std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| {
            format!(
                "cannot open lane reparse point '{}' to clear read-only: {error}",
                path.display()
            )
        })?;
    let info = FILE_BASIC_INFO {
        FileAttributes: attributes & !FILE_ATTRIBUTE_READONLY,
        ..Default::default()
    };
    let succeeded = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle() as _,
            FileBasicInfo,
            (&info as *const FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(format!(
            "cannot clear read-only attribute on lane reparse point '{}': {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn clear_readonly_link_attribute(_: &Path, _: &std::fs::Metadata) -> Result<(), String> {
    Ok(())
}
