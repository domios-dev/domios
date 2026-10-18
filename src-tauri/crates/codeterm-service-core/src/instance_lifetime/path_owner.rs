use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInstancePaths {
    data: PathBuf,
    config: PathBuf,
}

pub struct PathOwnership {
    identity: ResolvedInstancePaths,
    _locks: Vec<File>,
}

impl ResolvedInstancePaths {
    pub fn resolve(data: &Path, config: &Path) -> io::Result<Self> {
        Ok(Self {
            data: resolve_path(data)?,
            config: resolve_path(config)?,
        })
    }

    pub fn current() -> io::Result<Self> {
        let config = crate::paths::config_path().map_err(io::Error::other)?;
        let data = config
            .parent()
            .ok_or_else(|| io::Error::other("Config path has no data directory"))?;
        Self::resolve(data, &config)
    }

    pub fn data(&self) -> &Path {
        &self.data
    }

    pub fn config(&self) -> &Path {
        &self.config
    }

    pub fn try_acquire(self, registry: &Path) -> io::Result<PathOwnership> {
        create_registry(registry)?;
        let mut keys = vec![lock_key(&self.data)?, lock_key(&self.config)?];
        keys.sort_unstable();
        keys.dedup();
        let mut locks = Vec::new();
        for key in keys {
            let lock = open_lock(&registry.join(format!("{key}.lock")))?;
            match lock.try_lock() {
                Ok(()) => locks.push(lock),
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "Resolved instance paths already have an owner",
                    ));
                }
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        Ok(PathOwnership {
            identity: self,
            _locks: locks,
        })
    }
}

impl PathOwnership {
    pub fn identity(&self) -> &ResolvedInstancePaths {
        &self.identity
    }
}

fn resolve_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                if !resolved.pop() {
                    return Err(io::Error::other(
                        "Instance path escapes its filesystem root",
                    ));
                }
            }
            Component::CurDir => {}
            component => {
                resolved.push(component.as_os_str());
                match resolved.canonicalize() {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        if std::fs::symlink_metadata(&resolved).is_ok() {
                            return Err(io::Error::other("Instance path has a dangling alias"));
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

fn lock_key(path: &Path) -> io::Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("Instance path is not Unicode"))?;
    #[cfg(windows)]
    let path = crate::paths::strip_verbatim_prefix(path).to_lowercase();
    let mut hash = Sha256::new();
    hash.update(b"domios-path-owner-v1\0");
    hash.update(path.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

fn create_registry(root: &Path) -> io::Result<()> {
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    match builder.create(root) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::other("Ownership registry is not a directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::other(
                "Ownership registry must be private to its user",
            ));
        }
    }
    Ok(())
}

fn open_lock(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() || std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("Ownership lock is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::other(
                "Ownership lock must be private to its user",
            ));
        }
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
