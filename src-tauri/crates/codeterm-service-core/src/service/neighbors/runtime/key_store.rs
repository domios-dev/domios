use super::*;
use codeterm_neighbors::keys::ConfiguredKeyStore;

#[cfg(not(test))]
type RuntimeKeyStore = ConfiguredKeyStore<OsKeyStore>;

#[cfg(test)]
pub(super) enum RuntimeKeyStore {
    Configured(ConfiguredKeyStore<OsKeyStore>),
    Fixture(Arc<dyn codeterm_neighbors::keys::KeyStore>),
}

#[cfg(test)]
struct FixtureKeyStoreBinding(std::sync::Weak<dyn codeterm_neighbors::keys::KeyStore>);

#[cfg(test)]
#[derive(Default)]
struct MemoryKeyStore(parking_lot::Mutex<HashMap<String, Zeroizing<Vec<u8>>>>);

#[cfg(test)]
#[async_trait::async_trait]
impl codeterm_neighbors::keys::KeyStore for MemoryKeyStore {
    async fn load(
        &self,
        scope: &KeyScope,
        slot: codeterm_neighbors::keys::Slot,
    ) -> codeterm_neighbors::Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.0.lock().get(&scope.name(slot)).cloned())
    }

    async fn save(
        &self,
        scope: &KeyScope,
        slot: codeterm_neighbors::keys::Slot,
        bytes: Zeroizing<Vec<u8>>,
    ) -> codeterm_neighbors::Result<()> {
        self.0.lock().insert(scope.name(slot), bytes);
        Ok(())
    }

    async fn delete(
        &self,
        scope: &KeyScope,
        slot: codeterm_neighbors::keys::Slot,
    ) -> codeterm_neighbors::Result<()> {
        self.0.lock().remove(&scope.name(slot));
        Ok(())
    }
}

#[cfg(test)]
impl RuntimeKeyStore {
    fn store(&self) -> &dyn codeterm_neighbors::keys::KeyStore {
        match self {
            Self::Configured(store) => store,
            Self::Fixture(store) => store.as_ref(),
        }
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl codeterm_neighbors::keys::KeyStore for RuntimeKeyStore {
    async fn available(&self) -> bool {
        self.store().available().await
    }

    async fn load(
        &self,
        scope: &KeyScope,
        slot: codeterm_neighbors::keys::Slot,
    ) -> codeterm_neighbors::Result<Option<Zeroizing<Vec<u8>>>> {
        self.store().load(scope, slot).await
    }

    async fn save(
        &self,
        scope: &KeyScope,
        slot: codeterm_neighbors::keys::Slot,
        bytes: Zeroizing<Vec<u8>>,
    ) -> codeterm_neighbors::Result<()> {
        self.store().save(scope, slot, bytes).await
    }

    async fn delete(
        &self,
        scope: &KeyScope,
        slot: codeterm_neighbors::keys::Slot,
    ) -> codeterm_neighbors::Result<()> {
        self.store().delete(scope, slot).await
    }
}

impl ServiceContext {
    #[cfg(test)]
    pub(in crate::service::neighbors) fn fixture_neighbor_key_store(
        &self,
    ) -> Arc<dyn codeterm_neighbors::keys::KeyStore> {
        assert!(self.try_ext::<FixtureKeyStoreBinding>().is_none());
        let store: Arc<dyn codeterm_neighbors::keys::KeyStore> = Arc::new(MemoryKeyStore::default());
        self.extensions
            .get_or_init(|| FixtureKeyStoreBinding(Arc::downgrade(&store)));
        store
    }

    pub(super) fn neighbor_key_store(&self) -> codeterm_neighbors::Result<RuntimeKeyStore> {
        #[cfg(test)]
        if let Some(binding) = self.try_ext::<FixtureKeyStoreBinding>() {
            return binding
                .0
                .upgrade()
                .map(RuntimeKeyStore::Fixture)
                .ok_or(Error::SecretStore);
        }
        let file = self
            .ext::<crate::config::ConfigState>()
            .lock()
            .neighbors
            .key_store
            == Some(codeterm_types::config::neighbors::NeighborKeyStore::File);
        let root = if file {
            Some(self.account_data_root().map_err(|_| Error::SecretStore)?)
        } else {
            None
        };
        let store = ConfiguredKeyStore::new(OsKeyStore, root);
        #[cfg(test)]
        return Ok(RuntimeKeyStore::Configured(store));
        #[cfg(not(test))]
        Ok(store)
    }
}

#[cfg(test)]
mod tests;
