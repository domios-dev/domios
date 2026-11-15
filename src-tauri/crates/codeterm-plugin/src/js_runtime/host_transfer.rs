use super::host_api::PluginSandbox;
use crate::binary_transfer::{self as transfer, *};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

struct TransferConfig {
    pub descriptor: BinaryTransferDescriptor,
    pub bundle: PathBuf,
    pub permissions: super::manifest::PermissionsConfig,
}
#[derive(Default)]
pub(super) struct TransferRuntime {
    config: Option<TransferConfig>,
    pub(super) session: parking_lot::Mutex<Option<Arc<TransferSession>>>,
}
impl PluginSandbox {
    pub(super) fn with_binary_transfer(
        mut self,
        descriptor: Option<crate::binary_transfer::BinaryTransferDescriptor>,
        bundle: &Path,
        permissions: &super::manifest::PermissionsConfig,
    ) -> Self {
        self.transfer.config = descriptor.map(|descriptor| super::host_transfer::TransferConfig {
            descriptor,
            bundle: bundle.to_path_buf(),
            permissions: permissions.clone(),
        });
        self
    }
}
struct Authority {
    sandbox: Weak<PluginSandbox>,
}
impl Authority {
    fn sandbox(&self) -> Result<Arc<PluginSandbox>> {
        self.sandbox.upgrade().ok_or(TransferRefusal::NotAllowed)
    }
    fn permissions(&self, sandbox: &PluginSandbox) -> Result<super::manifest::PermissionsConfig> {
        let config = sandbox
            .transfer
            .config
            .as_ref()
            .ok_or(TransferRefusal::NotAllowed)?;
        let mut permissions = config.permissions.clone();
        super::grants::apply_persisted(&mut permissions, &config.bundle);
        Ok(permissions)
    }
}
impl TransferAuthority for Authority {
    fn authorize(&self, url: &reqwest::Url) -> Result<()> {
        let sandbox = self.sandbox()?;
        if !exact_grant(url, &self.permissions(&sandbox)?.network.allow) {
            return Err(TransferRefusal::NotAllowed);
        }
        Ok(())
    }
    fn readable(&self, path: &Path) -> bool {
        self.sandbox()
            .is_ok_and(|sandbox| sandbox.can_read(&path.to_string_lossy()))
    }
    fn credentials(&self, credential: &Credential) -> Result<BTreeMap<String, String>> {
        let sandbox = self.sandbox()?;
        if !self.permissions(&sandbox)?.secrets {
            return Err(TransferRefusal::NotAllowed);
        }
        match credential {
            Credential::Secret {
                name,
                header,
                prefix,
            } => {
                if name.is_empty()
                    || name.len() > 128
                    || header.is_empty()
                    || header.len() > 128
                    || prefix.as_ref().is_some_and(|prefix| prefix.len() > 64)
                {
                    return Err(TransferRefusal::InvalidRequest);
                }
                let value = sandbox
                    .service()
                    .and_then(|service| service.secret_get(sandbox.plugin_id(), name))
                    .ok_or(TransferRefusal::NotAllowed)?;
                if value.len() > 8192 {
                    return Err(TransferRefusal::NotAllowed);
                }
                Ok(BTreeMap::from([(
                    header.clone(),
                    format!("{}{value}", prefix.as_deref().unwrap_or("")),
                )]))
            }
            Credential::Declared { id, headers } => {
                if id.len() > 128
                    || headers.len() > 8
                    || headers
                        .iter()
                        .any(|(key, value)| key.len() > 128 || value.len() > 1024)
                {
                    return Err(TransferRefusal::InvalidRequest);
                }
                let prepared = super::host_credentials::inject(
                    sandbox.credentials(),
                    &serde_json::json!({"credential":{"id":id,"headers":headers}}).to_string(),
                )
                .map_err(|_| TransferRefusal::NotAllowed)?;
                let value: serde_json::Value =
                    serde_json::from_str(&prepared).map_err(|_| TransferRefusal::NotAllowed)?;
                serde_json::from_value(
                    value
                        .get("headers")
                        .cloned()
                        .unwrap_or(serde_json::json!({})),
                )
                .map_err(|_| TransferRefusal::NotAllowed)
            }
        }
    }
}
pub(super) fn session(sandbox: &Arc<PluginSandbox>) -> Result<Arc<TransferSession>> {
    let mut current = sandbox.transfer.session.lock();
    if let Some(session) = current.as_ref() {
        return Ok(session.clone());
    }
    let config = sandbox
        .transfer
        .config
        .as_ref()
        .ok_or(TransferRefusal::NotAllowed)?;
    let authority = Arc::new(Authority {
        sandbox: Arc::downgrade(sandbox),
    });
    let session = TransferSession::new(sandbox.plugin_id(), config.descriptor.clone(), authority)?;
    *current = Some(session.clone());
    Ok(session)
}
pub(super) fn activate(sandbox: &Arc<PluginSandbox>) {
    {
        let mut current = sandbox.transfer.session.lock();
        if current
            .as_ref()
            .is_some_and(|session| session.check().is_err())
        {
            *current = None;
        }
    }
    if let Ok(session) = session(sandbox) {
        transfer::activate(&session);
    }
}
pub(super) fn reload(sandbox: &Arc<PluginSandbox>) {
    let owned = sandbox
        .transfer
        .session
        .lock()
        .as_ref()
        .is_some_and(|session| crate::binary_transfer::is_current(session));
    revoke(sandbox);
    if owned {
        activate(sandbox);
    }
}
pub(super) fn revoke(sandbox: &PluginSandbox) {
    if let Some(session) = sandbox.transfer.session.lock().as_ref() {
        session.revoke();
        super::host_exec::drop_scoped_json_jobs(sandbox.plugin_id(), session.epoch());
    }
}
pub(super) fn deactivate(plugin: &str) {
    let session = crate::binary_transfer::live_session(plugin);
    transfer::deactivate(plugin);
    if let Some(session) = session {
        super::host_exec::drop_scoped_json_jobs(plugin, session.epoch());
    }
}

#[cfg(test)]
pub(super) fn fixture_session(
    sandbox: &Arc<PluginSandbox>,
    root: PathBuf,
    network: Arc<dyn crate::binary_transfer::TestNetwork>,
) -> Arc<TransferSession> {
    let descriptor = sandbox.transfer.config.as_ref().unwrap().descriptor.clone();
    let session = TransferSession::at_root(
        sandbox.plugin_id(),
        root,
        descriptor,
        Arc::new(Authority {
            sandbox: Arc::downgrade(sandbox),
        }),
    )
    .unwrap();
    *session.test_network.lock() = Some(network);
    *sandbox.transfer.session.lock() = Some(session.clone());
    transfer::activate(&session);
    session
}
