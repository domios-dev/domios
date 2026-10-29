use super::{KeyScope, KeyStore, Slot};
use crate::{Error, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct FileKeyStore {
    root: PathBuf,
}

impl FileKeyStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            root: data_dir.join("neighbor-keys"),
        }
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(PathBuf) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || operation(root))
            .await
            .map_err(|_| Error::SecretStore)?
    }
}

#[async_trait]
impl KeyStore for FileKeyStore {
    async fn load(&self, scope: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let name = scope.name(slot);
        self.run(move |root| platform::load(root, name)).await
    }
    async fn save(&self, scope: &KeyScope, slot: Slot, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        if bytes.len() != 64 {
            return Err(Error::InvalidKey);
        }
        let name = scope.name(slot);
        self.run(move |root| platform::save(root, name, bytes))
            .await
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()> {
        let name = scope.name(slot);
        self.run(move |root| platform::delete(root, name)).await
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::{
        ffi::CString,
        fs::{DirBuilder, File, OpenOptions},
        io::{Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        },
    };

    fn private(mode: u32, owner: u32, uid: u32, allowed: u32) -> Result<()> {
        if owner != uid || mode & 0o7777 & !allowed != 0 {
            return Err(Error::SecretStore);
        }
        Ok(())
    }
    fn directory(root: &std::path::Path) -> Result<File> {
        match DirBuilder::new().mode(0o700).create(root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Error::SecretStore),
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(root)
            .map_err(|_| Error::SecretStore)?;
        let metadata = file.metadata().map_err(|_| Error::SecretStore)?;
        private(
            metadata.mode(),
            metadata.uid(),
            unsafe { libc::geteuid() },
            0o700,
        )?;
        Ok(file)
    }
    fn open(directory: &File, name: &str, create: bool) -> Result<Option<File>> {
        let name = CString::new(name).map_err(|_| Error::InvalidKey)?;
        let safe = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        let flags = if create {
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
        } else {
            libc::O_RDONLY
        };
        let mut fd =
            unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags | safe, 0o600) };
        let created = create && fd >= 0;
        if create
            && fd < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists
        {
            fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), libc::O_RDWR | safe) };
        }
        if fd < 0 {
            return if !create
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
            {
                Ok(None)
            } else {
                Err(Error::SecretStore)
            };
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if created && unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(Error::SecretStore);
        }
        let metadata = file.metadata().map_err(|_| Error::SecretStore)?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(Error::SecretStore);
        }
        private(
            metadata.mode(),
            metadata.uid(),
            unsafe { libc::geteuid() },
            0o600,
        )?;
        Ok(Some(file))
    }
    pub fn load(root: PathBuf, name: String) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let directory = directory(&root)?;
        let Some(file) = open(&directory, &name, false)? else {
            return Ok(None);
        };
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(65)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::SecretStore)?;
        if bytes.len() != 64 {
            return Err(Error::InvalidKey);
        }
        Ok(Some(bytes))
    }
    pub fn save(root: PathBuf, name: String, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        let directory = directory(&root)?;
        let mut file = open(&directory, &name, true)?.ok_or(Error::SecretStore)?;
        file.write_all(&bytes)
            .and_then(|_| file.set_len(64))
            .and_then(|_| file.sync_all())
            .map_err(|_| Error::SecretStore)?;
        directory.sync_all().map_err(|_| Error::SecretStore)
    }
    pub fn delete(root: PathBuf, name: String) -> Result<()> {
        let directory = directory(&root)?;
        let Some(_file) = open(&directory, &name, false)? else {
            return Ok(());
        };
        let name = CString::new(name).map_err(|_| Error::InvalidKey)?;
        if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(Error::SecretStore);
        }
        directory.sync_all().map_err(|_| Error::SecretStore)
    }
    #[cfg(test)]
    mod tests;
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    pub fn load(_: PathBuf, _: String) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Err(Error::SecretStore)
    }
    pub fn save(_: PathBuf, _: String, _: Zeroizing<Vec<u8>>) -> Result<()> {
        Err(Error::SecretStore)
    }
    pub fn delete(_: PathBuf, _: String) -> Result<()> {
        Err(Error::SecretStore)
    }
}

#[cfg(test)]
mod tests;
