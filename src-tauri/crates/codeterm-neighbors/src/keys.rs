#[cfg(feature = "runtime")]
mod os;
#[cfg(feature = "runtime")]
pub use os::OsKeyStore;

use crate::crypto::{sha256_hex, PrivateKeys};
use crate::{Error, Result};
use async_trait::async_trait;
use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct KeyScope(String);

impl KeyScope {
    pub fn installation(instance: &str, uid: &str) -> Result<Self> {
        if instance.is_empty() || !crate::wire::uid(uid) {
            return Err(Error::InvalidKey);
        }
        let scope = serde_json::to_vec(&[instance, uid]).map_err(|_| Error::InvalidKey)?;
        Ok(Self(format!("domios-neighbor-{}", sha256_hex(&scope))))
    }

    pub fn name(&self, slot: Slot) -> String {
        format!(
            "{}-{}",
            self.0,
            match slot {
                Slot::Current => "current",
                Slot::Candidate => "candidate",
            }
        )
    }
}

#[derive(Clone, Copy)]
pub enum Slot {
    Current,
    Candidate,
}

#[async_trait]
pub trait KeyStore: Send + Sync {
    async fn load(&self, scope: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>>;
    async fn save(&self, scope: &KeyScope, slot: Slot, bytes: Zeroizing<Vec<u8>>) -> Result<()>;
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()>;
}

#[async_trait]
pub trait KeyRegistry: Send + Sync {
    async fn enroll(
        &self,
        signing_spki: &str,
        encryption_spki: &str,
        replace_others: bool,
    ) -> Result<String>;
    async fn revoke(&self, verifier_key_id: &str) -> Result<()>;
    async fn active_key_ids(&self) -> Result<Vec<String>>;
}

#[async_trait]
pub trait EnrollmentState: Send + Sync {
    async fn enable(&self) -> Result<()>;
}

pub struct InstallationKeys<'a> {
    pub store: &'a dyn KeyStore,
    pub registry: &'a dyn KeyRegistry,
    pub scope: KeyScope,
    pub uid: &'a str,
}

#[async_trait]
pub trait RecipientKeys: Send + Sync {
    async fn decrypt_for(
        &self,
        envelope: &crate::wire::Envelope,
        peer: &str,
    ) -> Result<Zeroizing<String>>;
}

#[async_trait]
impl RecipientKeys for PrivateKeys {
    async fn decrypt_for(
        &self,
        envelope: &crate::wire::Envelope,
        peer: &str,
    ) -> Result<Zeroizing<String>> {
        if self.encryption_key_id() != envelope.enc_key_ids.recipient {
            return Err(Error::LocalKeyUnavailable);
        }
        self.decrypt(envelope, peer)
    }
}

#[async_trait]
impl RecipientKeys for InstallationKeys<'_> {
    async fn decrypt_for(
        &self,
        envelope: &crate::wire::Envelope,
        peer: &str,
    ) -> Result<Zeroizing<String>> {
        let active = self.registry.active_key_ids().await?;
        for slot in [Slot::Candidate, Slot::Current] {
            if let Some(bytes) = self.store.load(&self.scope, slot).await? {
                let keys = PrivateKeys::from_storage(&bytes)?;
                if keys.encryption_key_id() == envelope.enc_key_ids.recipient {
                    if !active.contains(&keys.verifier_key_id(self.uid)) {
                        return Err(Error::Revoked);
                    }
                    return keys.decrypt(envelope, peer);
                }
            }
        }
        Err(Error::LocalKeyUnavailable)
    }
}

impl InstallationKeys<'_> {
    pub async fn enroll_and_enable(
        &self,
        confirmed: bool,
        rng: &mut (impl RngCore + CryptoRng),
        state: &dyn EnrollmentState,
    ) -> Result<PrivateKeys> {
        let keys = self.enroll(confirmed, rng).await?;
        state.enable().await?;
        Ok(keys)
    }
    pub async fn reconcile(&self) -> Result<Option<PrivateKeys>> {
        let current = self.load().await?;
        let candidate = self
            .store
            .load(&self.scope, Slot::Candidate)
            .await?
            .map(|bytes| PrivateKeys::from_storage(&bytes))
            .transpose()?;
        let active = self.registry.active_key_ids().await?;
        if let Some(candidate) = candidate {
            let id = candidate.verifier_key_id(self.uid);
            if active.contains(&id) {
                let previous = current
                    .as_ref()
                    .map(|keys| keys.verifier_key_id(self.uid))
                    .filter(|previous| previous != &id);
                if let Some(previous) = &previous {
                    if active.contains(previous) {
                        self.registry.revoke(previous).await?;
                    }
                }
                let confirmed = self.registry.active_key_ids().await?;
                if !confirmed.contains(&id)
                    || previous.is_some_and(|previous| confirmed.contains(&previous))
                {
                    return Err(Error::Transport);
                }
                self.store
                    .save(&self.scope, Slot::Current, candidate.storage_bytes())
                    .await?;
                self.store.delete(&self.scope, Slot::Candidate).await?;
                return Ok(Some(candidate));
            }
        }
        Ok(current.filter(|keys| active.contains(&keys.verifier_key_id(self.uid))))
    }
    pub async fn load(&self) -> Result<Option<PrivateKeys>> {
        self.store
            .load(&self.scope, Slot::Current)
            .await?
            .map(|bytes| PrivateKeys::from_storage(&bytes))
            .transpose()
    }

    pub async fn enroll(
        &self,
        takeover_confirmed: bool,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<PrivateKeys> {
        if !takeover_confirmed {
            return Err(Error::Gateway);
        }
        if self
            .store
            .load(&self.scope, Slot::Candidate)
            .await?
            .is_some()
        {
            return self.promote_candidate(true, rng).await;
        }
        if let Some(keys) = self.load().await? {
            let id = self
                .registry
                .enroll(&keys.signing_spki(), &keys.encryption_spki(), true)
                .await?;
            if id != keys.verifier_key_id(self.uid) {
                return Err(Error::InvalidKey);
            }
            return Ok(keys);
        }
        self.promote_candidate(true, rng).await
    }

    pub async fn rotate(&self, rng: &mut (impl RngCore + CryptoRng)) -> Result<PrivateKeys> {
        if self
            .store
            .load(&self.scope, Slot::Candidate)
            .await?
            .is_some()
        {
            return self.promote_candidate(false, rng).await;
        }
        let keys = self.load().await?.ok_or(Error::Revoked)?;
        if !self
            .registry
            .active_key_ids()
            .await?
            .contains(&keys.verifier_key_id(self.uid))
        {
            return Err(Error::Revoked);
        }
        self.promote_candidate(false, rng).await
    }

    async fn promote_candidate(
        &self,
        replace_others: bool,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<PrivateKeys> {
        let candidate = match self.store.load(&self.scope, Slot::Candidate).await? {
            Some(bytes) => PrivateKeys::from_storage(&bytes)?,
            None => {
                let candidate = PrivateKeys::generate(rng);
                self.store
                    .save(&self.scope, Slot::Candidate, candidate.storage_bytes())
                    .await?;
                candidate
            }
        };
        let id = self
            .registry
            .enroll(
                &candidate.signing_spki(),
                &candidate.encryption_spki(),
                replace_others,
            )
            .await?;
        if id != candidate.verifier_key_id(self.uid) {
            return Err(Error::InvalidKey);
        }
        drop(candidate);
        self.reconcile().await?.ok_or(Error::LocalKeyUnavailable)
    }

    pub async fn forget(&self) -> Result<()> {
        for slot in [Slot::Candidate, Slot::Current] {
            self.store.delete(&self.scope, slot).await?;
        }
        Ok(())
    }

    pub async fn revoke(&self) -> Result<()> {
        self.registry.active_key_ids().await?;
        let mut ids = Vec::new();
        for slot in [Slot::Candidate, Slot::Current] {
            if let Some(bytes) = self.store.load(&self.scope, slot).await? {
                let keys = PrivateKeys::from_storage(&bytes)?;
                ids.push(keys.verifier_key_id(self.uid));
            }
        }
        ids.sort();
        ids.dedup();
        for id in &ids {
            self.registry.revoke(id).await?;
        }
        if self
            .registry
            .active_key_ids()
            .await?
            .iter()
            .any(|id| ids.contains(id))
        {
            return Err(Error::Transport);
        }
        for slot in [Slot::Candidate, Slot::Current] {
            self.store.delete(&self.scope, slot).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
