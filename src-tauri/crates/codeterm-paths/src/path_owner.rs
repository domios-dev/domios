use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

pub use crate::instance_identity::ResolvedInstancePaths;

pub struct PathOwnership {
    identity: ResolvedInstancePaths,
    _locks: Vec<File>,
}

impl PathOwnership {
    pub fn try_acquire(
        identity: ResolvedInstancePaths,
        registry: &Path,
    ) -> io::Result<PathOwnership> {
        create_registry(registry)?;
        let mut keys = vec![lock_key(identity.data())?, lock_key(identity.config())?];
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
            identity,
            _locks: locks,
        })
    }

    pub fn identity(&self) -> &ResolvedInstancePaths {
        &self.identity
    }
}

fn lock_key(path: &Path) -> io::Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("Instance path is not Unicode"))?;
    #[cfg(windows)]
    let path = crate::strip_verbatim_prefix(path).to_lowercase();
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
