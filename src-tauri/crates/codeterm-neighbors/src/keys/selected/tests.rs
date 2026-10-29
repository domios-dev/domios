use super::*;

struct Os {
    available: bool,
}
#[async_trait]
impl KeyStore for Os {
    async fn available(&self) -> bool {
        self.available
    }
    async fn load(&self, _: &KeyScope, _: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        if self.available {
            Ok(Some(Zeroizing::new(vec![9; 64])))
        } else {
            Err(Error::SecretStore)
        }
    }
    async fn save(&self, _: &KeyScope, _: Slot, _: Zeroizing<Vec<u8>>) -> Result<()> {
        Err(Error::SecretStore)
    }
    async fn delete(&self, _: &KeyScope, _: Slot) -> Result<()> {
        Err(Error::SecretStore)
    }
}

#[tokio::test]
async fn unavailable_os_never_implicitly_selects_file() {
    let store = ConfiguredKeyStore::new(Os { available: false }, None);
    let scope = KeyScope::installation("instance", "owner").unwrap();
    assert!(store.load(&scope, Slot::Current).await.is_err());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn explicit_file_is_used_only_while_os_is_unavailable() {
    let data = tempfile::tempdir().unwrap();
    let scope = KeyScope::installation("instance", "owner").unwrap();
    let online = ConfiguredKeyStore::new(Os { available: true }, Some(data.path().into()));
    assert_eq!(
        online
            .load(&scope, Slot::Current)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        &[9; 64]
    );
    assert!(online
        .save(&scope, Slot::Current, Zeroizing::new(vec![1; 64]))
        .await
        .is_err());
    assert!(!data.path().join("neighbor-keys").exists());
    let offline = ConfiguredKeyStore::new(Os { available: false }, Some(data.path().into()));
    offline
        .save(&scope, Slot::Current, Zeroizing::new(vec![1; 64]))
        .await
        .unwrap();
    assert_eq!(
        offline
            .load(&scope, Slot::Current)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        &[1; 64]
    );
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn explicit_file_selection_is_refused_off_linux_even_if_os_works() {
    let data = tempfile::tempdir().unwrap();
    let scope = KeyScope::installation("instance", "owner").unwrap();
    let store = ConfiguredKeyStore::new(Os { available: true }, Some(data.path().into()));
    assert!(store.load(&scope, Slot::Current).await.is_err());
    assert!(!data.path().join("neighbor-keys").exists());
}
