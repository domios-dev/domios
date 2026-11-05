use super::membership::{signed_payload_limit, MembershipTrust, NOT_BEFORE_SKEW_SECONDS};
use super::state::{AccountMeshRefusal, RegistryObservation};
use super::wire::{MeshManagementRow, RegistryPresence};
use super::AccountMesh;
use codeterm_entitlements::DetachedGrant;
use serde::Deserialize;
use std::collections::BTreeSet;

pub const MAX_REGISTRY_BYTES: usize = 262_144;
pub const MAX_REGISTRY_MEMBERS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRegistry {
    pub grant: DetachedGrant,
    pub account_id: String,
    pub revision: u64,
    pub issued_at: u64,
    pub rows: Vec<MeshManagementRow>,
    pub revoked_installations: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audience {
    project: String,
    env: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema_version: u8,
    iss: String,
    aud: Audience,
    policy_epoch: String,
    account_id: String,
    revision: u64,
    issued_at: u64,
    members: Vec<MeshManagementRow>,
    revoked_installations: Vec<String>,
}

pub fn verify_registry(
    grant: &DetachedGrant,
    trust: &MembershipTrust,
    account: &str,
    now: u64,
) -> Result<VerifiedRegistry, AccountMeshRefusal> {
    let invalid = AccountMeshRefusal::Invalid;
    let value = signed_payload_limit(grant, trust, MAX_REGISTRY_BYTES).map_err(|_| invalid)?;
    let payload: Payload = serde_json::from_value(value).map_err(|_| invalid)?;
    if payload.schema_version != 1
        || payload.iss != "domios-mesh-registry"
        || payload.aud.project != trust.project()
        || payload.aud.env != trust.mode()
        || payload.policy_epoch != trust.policy_epoch()
        || payload.account_id != account
        || payload.revision == 0
        || payload.revision > 9_007_199_254_740_991
        || payload.issued_at == 0
        || payload.issued_at > now.saturating_add(NOT_BEFORE_SKEW_SECONDS)
        || payload.members.len() > MAX_REGISTRY_MEMBERS
        || payload.revoked_installations.len() > MAX_REGISTRY_MEMBERS
    {
        return Err(invalid);
    }
    let mut ids = BTreeSet::new();
    for row in &payload.members {
        if !codeterm_entitlements::text::valid_installation_id(&row.installation_id)
            || !ids.insert(row.installation_id.clone())
            || row.key_id.len() != 64
            || !row
                .key_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || row.key_generation == 0
            || row.slot_revision == 0
        {
            return Err(invalid);
        }
    }
    let mut revoked = BTreeSet::new();
    for id in payload.revoked_installations {
        if !codeterm_entitlements::text::valid_installation_id(&id) || !revoked.insert(id) {
            return Err(invalid);
        }
    }
    if payload
        .members
        .iter()
        .any(|row| row.active == revoked.contains(&row.installation_id))
    {
        return Err(invalid);
    }
    Ok(VerifiedRegistry {
        grant: grant.clone(),
        account_id: payload.account_id,
        revision: payload.revision,
        issued_at: payload.issued_at,
        rows: payload.members,
        revoked_installations: revoked,
    })
}

impl AccountMesh {
    pub async fn receive_registry_snapshot(
        &self,
        grant: &DetachedGrant,
    ) -> Result<bool, AccountMeshRefusal> {
        let state = self.state();
        let account = state
            .signed_in_account()
            .ok_or(AccountMeshRefusal::SignedOut)?;
        let trust = self
            .config
            .trust
            .as_ref()
            .ok_or(AccountMeshRefusal::VerificationUnavailable)?;
        let cached = self.with(|inner| inner.record.signed_registries.get(account).cloned());
        let received = grant.clone();
        let trust = trust.clone();
        let expected_account = account.to_owned();
        let now = self.now();
        let (verified, cached) = tokio::task::spawn_blocking(move || {
            let verified = verify_registry(&received, &trust, &expected_account, now)?;
            let cached = cached.and_then(|known| {
                if known == received {
                    Some(verified.clone())
                } else {
                    verify_registry(&known, &trust, &expected_account, now).ok()
                }
            });
            Ok::<_, AccountMeshRefusal>((verified, cached))
        })
        .await
        .map_err(|_| AccountMeshRefusal::Invalid)??;
        let accepted = self.with(|inner| {
            if !inner.is_current(state.account_epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.select_fences(
                account,
                cached.as_ref().map(|known| known.rows.as_slice()),
                &self.installation(),
            );
            if inner
                .state
                .signed_registry
                .as_ref()
                .is_some_and(|known| verified.revision <= known.revision)
            {
                return Ok(false);
            }
            if let Some(known) = cached {
                if verified.revision < known.revision
                    || (verified.revision == known.revision && verified.grant != known.grant)
                {
                    return Ok(false);
                }
            }
            if verified.rows.iter().any(|row| {
                inner
                    .record
                    .peer_keys
                    .get(&row.installation_id)
                    .is_some_and(|known| {
                        row.key_generation < known.generation
                            || (row.key_generation == known.generation
                                && row.key_id != known.key_id)
                    })
            }) {
                log::debug!("[account-mesh] registry refused: installation key rollback");
                return Err(AccountMeshRefusal::Invalid);
            }
            if verified.rows.iter().any(|row| {
                inner
                    .record
                    .peer_presence
                    .get(&row.installation_id)
                    .is_some_and(|known| {
                        row.key_generation == known.generation
                            && row.metadata_revision < known.metadata_revision
                    })
            }) {
                log::debug!("[account-mesh] registry refused: presence metadata rollback");
                return Err(AccountMeshRefusal::Invalid);
            }
            let freed_slot = inner.state.registry.as_ref().is_some_and(|previous| {
                previous.rows.iter().filter(|row| row.active).any(|old| {
                    !verified
                        .rows
                        .iter()
                        .any(|row| row.installation_id == old.installation_id && row.active)
                })
            });
            if freed_slot && inner.state.plan_limit_locked() {
                inner.resume_terminal();
                inner.retry = Default::default();
            }
            inner
                .record
                .signed_registries
                .insert(account.to_owned(), grant.clone());
            inner.observe_registry_keys(&verified.rows, &self.installation());
            inner
                .record
                .revoked_installations
                .extend(verified.revoked_installations.iter().cloned());
            if let Some(pending) = inner.record.local_revocations.get_mut(account) {
                pending.retain(|id| {
                    !verified.revoked_installations.contains(id)
                        && !verified
                            .rows
                            .iter()
                            .any(|row| &row.installation_id == id && !row.active)
                });
            }
            if let Some(local) = inner.record.local_revocations.get(account) {
                inner
                    .record
                    .revoked_installations
                    .extend(local.iter().cloned());
            }
            let revoked = inner.record.revoked_installations.clone();
            let own_allowed = verified.rows.iter().any(|row| {
                row.installation_id == self.installation()
                    && row.active
                    && row.eligibility == super::wire::Eligibility::Eligible
                    && row.presence == RegistryPresence::Present
                    && inner.state.membership.as_ref().is_none_or(|membership| {
                        row.key_id == membership.verified.key_id
                            && row.key_generation == membership.verified.key_generation
                            && row.slot_revision == membership.verified.slot_revision
                    })
            });
            inner.record.peer_presence_refresh = false;
            inner.resolved.clear();
            inner.update(&self.watch, |next| {
                next.revoked_installations = revoked;
                if !own_allowed {
                    next.membership = None;
                }
                next.registry = Some(RegistryObservation {
                    observed_at: self.now(),
                    registry_revision: verified.revision,
                    rows: verified.rows.clone(),
                    cursor: None,
                });
                next.registry_failure = None;
                next.signed_registry = Some(verified);
            });
            Ok(true)
        })?;
        if accepted {
            self.push().await;
            self.persist().await;
            self.ports.transport.registry_changed(grant).await;
        }
        Ok(accepted)
    }

    pub(super) async fn accept_server_registry(
        &self,
        grant: &DetachedGrant,
        account: &str,
        epoch: u64,
    ) -> Result<(), AccountMeshRefusal> {
        let trust = self
            .config
            .trust
            .as_ref()
            .ok_or(AccountMeshRefusal::VerificationUnavailable)?;
        if epoch == 0 {
            let received = grant.clone();
            let trust = trust.clone();
            let account = account.to_owned();
            let now = self.now();
            tokio::task::spawn_blocking(move || verify_registry(&received, &trust, &account, now))
                .await
                .map_err(|_| AccountMeshRefusal::Invalid)??;
            self.ports.transport.registry_changed(grant).await;
        } else {
            if !self.with(|inner| inner.is_current(epoch)) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            self.receive_registry_snapshot(grant).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
