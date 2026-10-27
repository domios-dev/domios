use super::{KeyScope, KeyStore, Slot};
use crate::{Error, Result};
use async_trait::async_trait;
use zeroize::Zeroizing;

pub struct OsKeyStore;

#[async_trait]
impl KeyStore for OsKeyStore {
    async fn load(&self, scope: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        platform::load(scope.name(slot)).await
    }
    async fn save(&self, scope: &KeyScope, slot: Slot, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        if bytes.len() != 64 {
            return Err(Error::SecretStore);
        }
        platform::save(scope.name(slot), bytes).await
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()> {
        platform::delete(scope.name(slot)).await
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::*,
    };

    fn target(name: &str) -> Vec<u16> {
        format!("Domios:Neighbors:{name}\0")
            .encode_utf16()
            .collect()
    }

    pub async fn load(name: String) -> Result<Option<Zeroizing<Vec<u8>>>> {
        tokio::task::spawn_blocking(move || {
            let target = target(&name);
            let mut credential = std::ptr::null_mut();
            unsafe {
                if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
                    return if GetLastError() == ERROR_NOT_FOUND {
                        Ok(None)
                    } else {
                        Err(Error::SecretStore)
                    };
                }
                let entry = &mut *credential;
                let result = if entry.CredentialBlobSize == 64 && !entry.CredentialBlob.is_null() {
                    let bytes = std::slice::from_raw_parts_mut(entry.CredentialBlob, 64);
                    let secret = Zeroizing::new(bytes.to_vec());
                    zeroize::Zeroize::zeroize(bytes);
                    Ok(Some(secret))
                } else {
                    Err(Error::SecretStore)
                };
                CredFree(credential.cast());
                result
            }
        })
        .await
        .map_err(|_| Error::SecretStore)?
    }

    pub async fn save(name: String, mut bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        tokio::task::spawn_blocking(move || {
            let mut target = target(&name);
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_mut_ptr(),
                CredentialBlobSize: bytes.len() as u32,
                CredentialBlob: bytes.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                ..Default::default()
            };
            if unsafe { CredWriteW(&credential, 0) } == 0 {
                Err(Error::SecretStore)
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|_| Error::SecretStore)?
    }

    pub async fn delete(name: String) -> Result<()> {
        tokio::task::spawn_blocking(move || {
            if unsafe { CredDeleteW(target(&name).as_ptr(), CRED_TYPE_GENERIC, 0) } != 0
                || unsafe { GetLastError() } == ERROR_NOT_FOUND
            {
                Ok(())
            } else {
                Err(Error::SecretStore)
            }
        })
        .await
        .map_err(|_| Error::SecretStore)?
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    const SERVICE: &str = "Domios:Neighbors";

    pub async fn load(name: String) -> Result<Option<Zeroizing<Vec<u8>>>> {
        tokio::task::spawn_blocking(move || match get_generic_password(SERVICE, &name) {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(_) => Err(Error::SecretStore),
        })
        .await
        .map_err(|_| Error::SecretStore)?
    }
    pub async fn save(name: String, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        tokio::task::spawn_blocking(move || {
            set_generic_password(SERVICE, &name, &bytes).map_err(|_| Error::SecretStore)
        })
        .await
        .map_err(|_| Error::SecretStore)?
    }
    pub async fn delete(name: String) -> Result<()> {
        tokio::task::spawn_blocking(move || match delete_generic_password(SERVICE, &name) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(_) => Err(Error::SecretStore),
        })
        .await
        .map_err(|_| Error::SecretStore)?
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use secret_service::{EncryptionType, SecretService};
    use std::{collections::HashMap, future::Future, time::Duration};

    fn attributes(name: &str) -> HashMap<&str, &str> {
        HashMap::from([("service", "Domios:Neighbors"), ("account", name)])
    }

    async fn bounded<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::time::timeout(Duration::from_secs(15), future)
            .await
            .map_err(|_| Error::SecretStore)?
    }

    pub async fn load(name: String) -> Result<Option<Zeroizing<Vec<u8>>>> {
        bounded(async {
            let service = SecretService::connect(EncryptionType::Dh)
                .await
                .map_err(|_| Error::SecretStore)?;
            let items = service
                .search_items(attributes(&name))
                .await
                .map_err(|_| Error::SecretStore)?;
            if !items.locked.is_empty() || items.unlocked.len() > 1 {
                return Err(Error::SecretStore);
            }
            match items.unlocked.first() {
                Some(item) => item
                    .get_secret()
                    .await
                    .map(Zeroizing::new)
                    .map(Some)
                    .map_err(|_| Error::SecretStore),
                None => Ok(None),
            }
        })
        .await
    }
    pub async fn save(name: String, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        bounded(async {
            let service = SecretService::connect(EncryptionType::Dh)
                .await
                .map_err(|_| Error::SecretStore)?;
            let collection = service
                .get_default_collection()
                .await
                .map_err(|_| Error::SecretStore)?;
            if collection
                .is_locked()
                .await
                .map_err(|_| Error::SecretStore)?
            {
                return Err(Error::SecretStore);
            }
            collection
                .create_item(
                    "Domios Neighbor installation keys",
                    attributes(&name),
                    &bytes,
                    true,
                    "application/octet-stream",
                )
                .await
                .map_err(|_| Error::SecretStore)?;
            Ok(())
        })
        .await
    }
    pub async fn delete(name: String) -> Result<()> {
        bounded(async {
            let service = SecretService::connect(EncryptionType::Dh)
                .await
                .map_err(|_| Error::SecretStore)?;
            let items = service
                .search_items(attributes(&name))
                .await
                .map_err(|_| Error::SecretStore)?;
            if !items.locked.is_empty() {
                return Err(Error::SecretStore);
            }
            for item in items.unlocked {
                item.delete().await.map_err(|_| Error::SecretStore)?;
            }
            Ok(())
        })
        .await
    }
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub async fn load(_: String) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Err(Error::SecretStore)
    }
    pub async fn save(_: String, _: Zeroizing<Vec<u8>>) -> Result<()> {
        Err(Error::SecretStore)
    }
    pub async fn delete(_: String) -> Result<()> {
        Err(Error::SecretStore)
    }
}
