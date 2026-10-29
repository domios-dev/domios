use super::*;
use codeterm_neighbors::keys::ConfiguredKeyStore;

impl ServiceContext {
    pub(super) fn neighbor_key_store(
        &self,
    ) -> codeterm_neighbors::Result<ConfiguredKeyStore<OsKeyStore>> {
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
        Ok(ConfiguredKeyStore::new(OsKeyStore, root))
    }
}
