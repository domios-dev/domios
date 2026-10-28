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
        self.queue_neighbor_logout_with_metadata(session, &metadata)
            .await
    }

    pub(super) async fn queue_neighbor_logout_with_metadata(
        &self,
        session: &codeterm_account::Session,
        metadata: &Metadata,
    ) -> Result<(), String> {
        let persistence = match metadata.settings().await {
            Ok(mut settings) => {
                self.remember_neighbor_concierge_accounts(&session.identity.uid, &settings);
                settings.enabled = false;
                settings.cleanup_pending = true;
                metadata
                    .save_settings(settings)
                    .await
                    .map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        };
        let retirement = self
            .retire_local_neighbor_concierges(
                metadata,
                &session.identity.uid,
                self.neighbor_history_generation(),
            )
            .await;
        match (persistence, retirement) {
            (Err(persistence), Err(retirement)) => {
                Err(format!("{persistence}; concierge retirement: {retirement}"))
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
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
        let native = OsKeyStore;
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
