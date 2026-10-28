use super::*;
use codeterm_neighbors::keys::KeyStore;

pub(crate) struct LogoutKeyPorts {
    pub store: Arc<dyn KeyStore>,
    pub registry: Arc<dyn KeyRegistry>,
}

impl ServiceContext {
    pub(crate) async fn queue_neighbor_logout(
        &self,
        session: &codeterm_account::Session,
    ) -> Result<(), String> {
        if session.identity.provider == "local" {
            return Ok(());
        }
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let mut settings = metadata.settings().await.map_err(|e| e.to_string())?;
        self.remember_neighbor_concierge_accounts(&session.identity.uid, &settings);
        settings.enabled = false;
        settings.cleanup_pending = true;
        metadata
            .save_settings(settings)
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn retry_neighbor_logout(
        &self,
        metadata: &Metadata,
        scope: &KeyScope,
        uid: &str,
        registry: &dyn KeyRegistry,
    ) -> Result<(), String> {
        let mut settings = metadata.settings().await.map_err(|e| e.to_string())?;
        if !settings.cleanup_pending {
            return Ok(());
        }
        let native = self.neighbor_key_store().map_err(|e| e.to_string())?;
        let (store, registry): (&dyn KeyStore, &dyn KeyRegistry) =
            match self.try_ext::<LogoutKeyPorts>() {
                Some(ports) => (ports.store.as_ref(), ports.registry.as_ref()),
                None => (&native, registry),
            };
        let result = InstallationKeys {
            store,
            registry,
            scope: scope.clone(),
            uid,
        }
        .forget()
        .await;
        if let Err(error) = result {
            log::warn!("Account sign-out cleanup failed: stage=neighbor_keys error_kind={error:?}");
            return Err(error.to_string());
        }
        settings.enabled = false;
        settings.cleanup_pending = false;
        metadata
            .save_settings(settings)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
