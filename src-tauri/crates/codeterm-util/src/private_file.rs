use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[cfg(windows)]
mod windows;

pub fn set_process_creation_mask() {
    #[cfg(unix)]
    unsafe {
        libc::umask(0o077);
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
        windows::restrict(path)
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
        windows::restrict_dir(path)
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
        Ok(()) => restrict_dir(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn tighten_tree(root: &Path) -> io::Result<()> {
    create_dir_all(root)?;
    restrict_dir(root)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            tighten_tree(&path)?;
        } else if kind.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let executable = entry.metadata()?.permissions().mode() & 0o111 != 0;
                let state = matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("toml" | "yaml" | "yml" | "json" | "db" | "sqlite" | "bak" | "log")
                ) || path.file_name().is_some_and(|name| name == ".secrets");
                let mode = if executable && !state { 0o700 } else { 0o600 };
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            }
            #[cfg(windows)]
            restrict(&path)?;
        }
    }
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let tmp = crate::atomic_write::unique_tmp_path(path);
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        restrict(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests;
