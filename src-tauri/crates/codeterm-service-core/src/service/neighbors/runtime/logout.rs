use super::*;
use codeterm_neighbors::keys::KeyStore;

pub(crate) struct LogoutKeyPorts {
    pub store: Arc<dyn KeyStore>,
    pub registry: Arc<dyn KeyRegistry>,
}

impl ServiceContext {
    pub(crate) async fn revoke_neighbors_owned(&self) -> Result<(), String> {
        self.neighbor_identity_changed();
        let account = self.account().await?;
        let Some(session) = account.session().await else {
            return Ok(());
        };
        if account.status().await.provider == "local" {
            return Ok(());
        }
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let mut settings = metadata.settings().await.map_err(|e| e.to_string())?;
        let native;
        let ports = match self.try_ext::<LogoutKeyPorts>() {
            Some(ports) => ports,
            None => {
                let (registry, _, _) = self.neighbor_key_ports().await?;
                native = LogoutKeyPorts {
                    store: Arc::new(OsKeyStore),
                    registry,
                };
                &native
            }
        };
        let active = ports
            .registry
            .active_key_ids()
            .await
            .map_err(|e| e.to_string())?;
        if settings.enabled || !active.is_empty() {
            let scope = KeyScope::installation(
                self.installation_id().await?.as_str(),
                &session.identity.uid,
            )
            .map_err(|e| e.to_string())?;
            InstallationKeys {
                store: ports.store.as_ref(),
                registry: ports.registry.as_ref(),
                scope,
                uid: &session.identity.uid,
            }
            .revoke()
            .await
            .map_err(|e| e.to_string())?;
        }
        settings.enabled = false;
        metadata
            .save_settings(settings)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
