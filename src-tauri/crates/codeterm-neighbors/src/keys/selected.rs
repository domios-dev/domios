use super::{FileKeyStore, KeyScope, KeyStore, Slot};
use crate::{Error, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use zeroize::Zeroizing;

pub struct ConfiguredKeyStore<S> {
    os: S,
    file: Option<FileKeyStore>,
}

impl<S: KeyStore> ConfiguredKeyStore<S> {
    pub fn new(os: S, explicit_file_root: Option<PathBuf>) -> Self {
        Self {
            os,
            file: explicit_file_root.map(FileKeyStore::new),
        }
    }

    async fn selected(&self) -> Result<&dyn KeyStore> {
        if self.file.is_none() {
            return Ok(&self.os);
        }
        #[cfg(not(target_os = "linux"))]
        if self.file.is_some() {
            return Err(Error::SecretStore);
        }
        if self.os.available().await {
            return Ok(&self.os);
        }
        self.file
            .as_ref()
            .map(|store| store as &dyn KeyStore)
            .ok_or(Error::SecretStore)
    }
}

#[async_trait]
impl<S: KeyStore> KeyStore for ConfiguredKeyStore<S> {
    async fn load(&self, scope: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        self.selected().await?.load(scope, slot).await
    }
    async fn save(&self, scope: &KeyScope, slot: Slot, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        self.selected().await?.save(scope, slot, bytes).await
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()> {
        self.selected().await?.delete(scope, slot).await
    }
}

#[cfg(test)]
mod tests;
