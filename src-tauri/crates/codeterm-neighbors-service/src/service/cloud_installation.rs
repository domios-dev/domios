pub mod controller;
mod creation;
mod renewal;
#[cfg(test)]
mod tests;
use super::account::credentials::MESH_INSTALLATION_KEY;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codeterm_account::{
    account_mesh::{
        cloud_enrollment::{
            fixed_bytes, member_identity, verify_apply, Apply, Discovery, ExpectedApply,
            MAX_ENROLL_APPLY_BYTES,
        },
        AccountMesh, InstallationKey, MembershipTrust,
    },
    protected_credentials::ProtectedCredentials,
    Account, FileInstallationStore, Identity, Secret,
};
use codeterm_entitlements::DetachedGrant;
use codeterm_mesh::cloud_ingress::{
    BootstrapResponder, ChannelProof, CloudIngressAuthority, EnrollmentError, EnrollmentHandler,
    EnrollmentOperation, EnrollmentResult, EnrollmentSession, IngressError,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};
use zeroize::Zeroizing;

const RECORD: &str = "domios-cloud-enrollment-v1";

#[derive(Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
enum Stored {
    Bound {
        controller: String,
        nonce: String,
    },
    Enrolled {
        account_id: String,
        discovery: Discovery,
        membership: DetachedGrant,
        signed_registry: DetachedGrant,
    },
}

enum Phase {
    Bound {
        controller: [u8; 32],
        nonce: Zeroizing<[u8; 32]>,
    },
    Enrolled {
        account_id: String,
        membership: DetachedGrant,
        signed_registry: DetachedGrant,
    },
    Unavailable,
}

struct Inner {
    renewal: Arc<tokio::sync::Mutex<()>>,
    lifecycle: tokio::sync::watch::Sender<Option<Arc<AccountMesh>>>,
    phase: Mutex<Phase>,
    store: ProtectedCredentials,
    root: std::path::PathBuf,
    signer: InstallationKey,
    discovery: Discovery,
    trust: MembershipTrust,
}

#[derive(Clone)]
pub struct CloudInstallation(Arc<Inner>);

impl CloudInstallation {
    pub fn open(
        root: &Path,
        mesh_instance_id: String,
        mesh_public_key: [u8; 32],
    ) -> Result<Self, String> {
        Self::restore(root, mesh_instance_id, mesh_public_key, creation::take()?)
    }

    fn restore(
        root: &Path,
        mesh_instance_id: String,
        mesh_public_key: [u8; 32],
        supplied: Option<(String, Zeroizing<String>)>,
    ) -> Result<Self, String> {
        let unavailable = || "Cloud installation identity unavailable".to_owned();
        let store = ProtectedCredentials::new(root);
        let existing = store.load(RECORD).map_err(|_| unavailable())?;
        let signer = match store
            .load(MESH_INSTALLATION_KEY)
            .map_err(|_| unavailable())?
        {
            Some(key) => InstallationKey::from_secret(&key).ok_or_else(unavailable)?,
            None if existing.is_none() => {
                let key = InstallationKey::generate();
                store
                    .save_persistent(MESH_INSTALLATION_KEY, &key.to_secret())
                    .map_err(|_| unavailable())?;
                key
            }
            None => return Err(unavailable()),
        };
        let installation = Account::installation_id(&FileInstallationStore::new(root.to_owned()))
            .map_err(|_| unavailable())?;
        let discovery = Discovery {
            cloud_installation_id: installation.as_str().into(),
            cloud_installation_key: URL_SAFE_NO_PAD.encode(signer.public_bytes()),
            mesh_instance_id,
            mesh_public_key: URL_SAFE_NO_PAD.encode(mesh_public_key),
        };
        let stored = match existing {
            Some(secret) => serde_json::from_str(secret.expose()).map_err(|_| unavailable())?,
            None => {
                let (controller, nonce) = supplied.as_ref().ok_or_else(unavailable)?;
                if fixed_bytes::<32>(controller).is_none() || fixed_bytes::<32>(nonce).is_none() {
                    return Err(unavailable());
                }
                let bound = Stored::Bound {
                    controller: controller.clone(),
                    nonce: nonce.to_string(),
                };
                Self::persist(&store, root, &bound).map_err(|_| unavailable())?;
                bound
            }
        };
        let phase = match stored {
            Stored::Bound { controller, nonce } => Phase::Bound {
                controller: fixed_bytes(&controller).ok_or_else(unavailable)?,
                nonce: Zeroizing::new(fixed_bytes(&nonce).ok_or_else(unavailable)?),
            },
            Stored::Enrolled {
                account_id,
                discovery: recorded,
                membership,
                signed_registry,
            } if recorded == discovery => Phase::Enrolled {
                account_id,
                membership,
                signed_registry,
            },
            Stored::Enrolled { .. } => return Err(unavailable()),
        };
        Ok(Self(Arc::new(Inner {
            renewal: Default::default(),
            lifecycle: tokio::sync::watch::channel(None).0,
            phase: Mutex::new(phase),
            store,
            root: root.into(),
            signer,
            discovery,
            trust: MembershipTrust::new(codeterm_entitlements::pinned()),
        })))
    }

    fn persist(
        store: &ProtectedCredentials,
        root: &Path,
        value: &Stored,
    ) -> Result<(), EnrollmentError> {
        let text = serde_json::to_string(value).map_err(|_| EnrollmentError::Unavailable)?;
        store
            .save_persistent(RECORD, &Secret::new(text))
            .map_err(|_| EnrollmentError::Unavailable)?;
        #[cfg(unix)]
        std::fs::File::open(root)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| EnrollmentError::Unavailable)?;
        #[cfg(not(unix))]
        let _ = root;
        Ok(())
    }

    pub fn installation_id(&self) -> &str {
        &self.0.discovery.cloud_installation_id
    }

    pub fn identity(&self) -> Option<Identity> {
        let now = codeterm_service_core::service::neighbor_account::now();
        let live = self.0.lifecycle.borrow().as_ref()?.state();
        let current = live.current_membership(now)?;
        if live.revoked_installations.contains(self.installation_id())
            || !matches!(&*self.0.phase.lock(), Phase::Enrolled { account_id, .. } if account_id == &current.verified.account_id)
        {
            return None;
        }
        let membership =
            codeterm_account::account_mesh::parse_membership_token(&current.token).ok()?;
        member_identity(
            &membership,
            &self.0.trust,
            &current.verified.account_id,
            &self.0.discovery,
            now,
        )
    }

    pub(super) async fn bind_lifecycle(
        &self,
        mesh: Arc<AccountMesh>,
    ) -> Result<(), EnrollmentError> {
        if !self.0.lifecycle.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(mesh);
            true
        }) {
            return Err(EnrollmentError::Unavailable);
        }
        self.publish().await
    }

    async fn publish(&self) -> Result<(), EnrollmentError> {
        let (account, membership, registry) = match &*self.0.phase.lock() {
            Phase::Bound { .. } => return Ok(()),
            Phase::Unavailable => return Err(EnrollmentError::Unavailable),
            Phase::Enrolled {
                account_id,
                membership,
                signed_registry,
            } => (
                account_id.clone(),
                membership.clone(),
                signed_registry.clone(),
            ),
        };
        let mesh = self
            .0
            .lifecycle
            .borrow()
            .clone()
            .ok_or(EnrollmentError::Unavailable)?;
        mesh.publish_cloud_membership(&account, &membership, &registry)
            .await
            .map_err(|_| EnrollmentError::Rejected)
    }

    fn apply(
        &self,
        controller: [u8; 32],
        binding: [u8; 32],
        payload: &[u8],
    ) -> Result<Vec<u8>, EnrollmentError> {
        let apply: Apply =
            serde_json::from_slice(payload).map_err(|_| EnrollmentError::InvalidFrame)?;
        let mut state = self.0.phase.lock();
        let (bound, nonce) = match &*state {
            Phase::Bound { controller, nonce } => (controller, nonce),
            Phase::Enrolled { .. } => return Err(EnrollmentError::AlreadyApplied),
            Phase::Unavailable => return Err(EnrollmentError::Unavailable),
        };
        if controller != *bound {
            return Err(EnrollmentError::Rejected);
        }
        let expected = ExpectedApply {
            local: &self.0.discovery,
            installation_key_id: &self.0.signer.key_id(),
            controller: bound,
            channel_binding: &binding,
            nonce,
        };
        let membership = verify_apply(
            &apply,
            &expected,
            &self.0.trust,
            codeterm_service_core::service::neighbor_account::now(),
        )
        .ok_or(EnrollmentError::Rejected)?;
        let receipt = Stored::Enrolled {
            account_id: membership.account_id.clone(),
            discovery: self.0.discovery.clone(),
            membership: apply.reply.membership.clone(),
            signed_registry: apply.reply.signed_registry.clone(),
        };
        *state = Phase::Unavailable;
        drop(state);
        Self::persist(&self.0.store, &self.0.root, &receipt)?;
        *self.0.phase.lock() = Phase::Enrolled {
            account_id: membership.account_id,
            membership: apply.reply.membership,
            signed_registry: apply.reply.signed_registry,
        };
        serde_json::to_vec(&serde_json::json!({"signed_in":true,"installation_id":self.0.discovery.cloud_installation_id})).map_err(|_|EnrollmentError::Unavailable)
    }
}

impl CloudIngressAuthority for CloudInstallation {
    fn bootstrap_responder(&self) -> Result<BootstrapResponder, IngressError> {
        match &*self.0.phase.lock() {
            Phase::Bound { nonce, .. } => BootstrapResponder::new(nonce),
            _ => Err(IngressError::Rejected),
        }
    }
    fn installation_public_key(&self) -> Result<[u8; 32], IngressError> {
        Ok(self.0.signer.public_bytes())
    }
    fn sign_channel_proof(&self, proof: &ChannelProof) -> Result<[u8; 64], IngressError> {
        self.0
            .signer
            .sign_cloud_channel_proof(proof.as_bytes())
            .ok_or(IngressError::Rejected)
    }
    fn authorize_bootstrap_controller(
        &self,
        key: &[u8; 32],
        _: &[u8; 32],
    ) -> Result<(), IngressError> {
        match &*self.0.phase.lock() {
            Phase::Bound { controller, .. } if controller == key => Ok(()),
            _ => Err(IngressError::Rejected),
        }
    }
}

impl EnrollmentHandler for CloudInstallation {
    fn handle<'a>(
        &'a self,
        session: &'a EnrollmentSession,
        op: EnrollmentOperation,
        payload: &'a [u8],
    ) -> EnrollmentResult<'a> {
        Box::pin(async move {
            if session.local_installation_key() != &self.0.signer.public_bytes() {
                return Err(EnrollmentError::Rejected);
            }
            let mut lifecycle = self.0.lifecycle.subscribe();
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                lifecycle.wait_for(Option::is_some),
            )
            .await
            .map_err(|_| EnrollmentError::Unavailable)?
            .map_err(|_| EnrollmentError::Unavailable)?;
            let remote = *session.remote_installation_key();
            let binding = *session.channel_binding();
            match op {
                EnrollmentOperation::Apply => {
                    if payload.len() > MAX_ENROLL_APPLY_BYTES {
                        return Err(EnrollmentError::InvalidFrame);
                    }
                    let owner = self.clone();
                    let bytes = payload.to_vec();
                    let publisher = self.clone();
                    // Once Apply starts, finish publication even if its connection closes.
                    tokio::spawn(async move {
                        let ack = tokio::task::spawn_blocking(move || {
                            owner.apply(remote, binding, &bytes)
                        })
                        .await
                        .map_err(|_| EnrollmentError::Unavailable)??;
                        publisher.publish().await?;
                        Ok(ack)
                    })
                    .await
                    .map_err(|_| EnrollmentError::Unavailable)?
                }
                _ if !payload.is_empty() => Err(EnrollmentError::InvalidFrame),
                op => {
                    let state = self.0.phase.lock();
                    if !matches!(&*state,Phase::Bound {controller,..} if *controller==remote) {
                        return Err(EnrollmentError::Rejected);
                    }
                    match op {
                        EnrollmentOperation::Discover => serde_json::to_vec(&self.0.discovery)
                            .map_err(|_| EnrollmentError::Unavailable),
                        EnrollmentOperation::Status => Ok(br#"{"signed_in":false}"#.to_vec()),
                        EnrollmentOperation::Apply => Err(EnrollmentError::InvalidFrame),
                    }
                }
            }
        })
    }
}
