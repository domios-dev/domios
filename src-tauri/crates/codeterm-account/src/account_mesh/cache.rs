use super::lifecycle::{AccountMesh, Inner};
use super::membership::{parse_membership_token, verify_membership, MembershipExpectation};
use super::operations::Signed;
use super::state::{Publication, RegistryObservation, SelfInstallation, SignedMembership};
use super::wire::{MeshEndpoint, MeshEndpointWire, RegistryPresence};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionCache {
    token: String,
    issued_at: u64,
    own: SelfInstallation,
    registry: Option<RegistryObservation>,
    presence: Option<RegistryPresence>,
    endpoint: Option<MeshEndpointWire>,
    published_until: u64,
    publication_refresh_after: u64,
    sync_pending: bool,
}

impl Inner {
    pub(super) fn cache_admission(&mut self) {
        if self.record.signed_out {
            self.record.admission_cache = None;
            return;
        }
        let (Some(membership), Some(own)) = (&self.state.membership, &self.state.self_installation)
        else {
            return;
        };
        self.record.admission_cache = Some(AdmissionCache {
            token: membership.token.clone(),
            issued_at: membership.verified.issued_at,
            own: own.clone(),
            registry: self.state.registry.clone(),
            presence: self.registry_presence,
            endpoint: self.published_endpoint.as_ref().map(MeshEndpoint::wire),
            published_until: self.published_until,
            publication_refresh_after: self.publication_refresh_after,
            sync_pending: self.sync_pending,
        });
    }
}

impl AccountMesh {
    pub(super) fn cached_active_target(&self, signed: &Signed) -> Option<super::wire::MeshTarget> {
        let cache = self.with(|inner| inner.record.admission_cache.clone())?;
        let grant = parse_membership_token(&cache.token).ok()?;
        let verified = verify_membership(
            &grant,
            &signed.trust,
            &MembershipExpectation {
                account_id: &signed.account,
                installation_id: Some(&self.installation()),
                key_id: Some(&cache.own.key_id),
                key_generation: Some(cache.own.key_generation),
                slot_revision: Some(cache.own.slot_revision),
            },
            cache.issued_at,
        )
        .ok()?;
        cache
            .registry?
            .rows
            .into_iter()
            .find(|row| {
                row.active
                    && row.installation_id == verified.installation_id
                    && row.key_id == verified.key_id
                    && row.key_generation == verified.key_generation
                    && row.slot_revision == verified.slot_revision
            })
            .map(|row| row.target())
    }

    pub(super) async fn restore_admission(
        &self,
        signed: &Signed,
    ) -> Result<(), super::AccountMeshRefusal> {
        let candidate = self.with(|inner| {
            if inner.state.membership.is_some() || inner.record.signed_out {
                return None;
            }
            Some((
                inner.record.admission_cache.clone()?,
                inner.record.keys.get(&signed.account)?.clone(),
                *inner.record.slots.get(&signed.account)?,
            ))
        });
        let Some((cache, binding, slot)) = candidate else {
            return Ok(());
        };
        if binding.key_id != signed.key.key_id() {
            return Ok(());
        }
        let Ok(grant) = parse_membership_token(&cache.token) else {
            return Ok(());
        };
        let Ok(verified) = verify_membership(
            &grant,
            &signed.trust,
            &MembershipExpectation {
                account_id: &signed.account,
                installation_id: Some(&self.installation()),
                key_id: Some(&binding.key_id),
                key_generation: Some(binding.generation),
                slot_revision: Some(slot),
            },
            cache.issued_at,
        ) else {
            return Ok(());
        };
        let long_lease = verified.is_current(self.now())
            && verified.expires_at.saturating_sub(verified.issued_at) > 3600;
        let restored = self.with(|inner| {
            if !inner.is_current(signed.epoch)
                || inner
                    .record
                    .revoked_installations
                    .contains(&verified.installation_id)
            {
                return false;
            }
            inner.metadata_revision = Some(cache.own.metadata_revision);
            inner.registry_presence = cache.presence;
            inner.sync_pending = cache.sync_pending;
            inner.published_endpoint = cache.endpoint.as_ref().and_then(|endpoint| {
                MeshEndpoint::parse(&endpoint.tailnet_ip, endpoint.api_port)
                    .map(|parsed| parsed.with_transport(endpoint.transport.clone()))
            });
            inner.published_until = cache.published_until;
            inner.publication_refresh_after = cache.publication_refresh_after;
            let publication =
                if inner.published_endpoint.is_some() && cache.published_until > self.now() {
                    Publication::Published
                } else {
                    Publication::NotReady
                };
            inner.update(&self.watch, |state| {
                state.membership_epoch += 1;
                state.self_installation = Some(SelfInstallation {
                    installation_id: verified.installation_id.clone(),
                    key_id: verified.key_id.clone(),
                    key_generation: verified.key_generation,
                    slot_revision: verified.slot_revision,
                    metadata_revision: cache.own.metadata_revision,
                    display_name: cache.own.display_name,
                });
                state.membership = Some(SignedMembership {
                    token: cache.token,
                    verified,
                });
                state.registry = cache.registry;
                state.publication = publication;
            });
            true
        });
        if restored && long_lease {
            if let Err(refusal) = self.read_registry(signed).await {
                let authoritative = matches!(
                    refusal,
                    super::AccountMeshRefusal::Forbidden
                        | super::AccountMeshRefusal::Revoked
                        | super::AccountMeshRefusal::KeyConflict
                );
                if authoritative {
                    self.with(|inner| {
                        if inner.is_current(signed.epoch) {
                            inner.update(&self.watch, |state| state.membership = None);
                        }
                    });
                    return Ok(());
                }
                return Err(refusal);
            }
        }
        Ok(())
    }
}
