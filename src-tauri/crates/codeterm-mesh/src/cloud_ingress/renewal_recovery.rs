use super::{
    dial,
    enrolled_wire::{ChannelIo, EncryptedStream},
    CloudRoute, IngressError, RenewalReceiver,
};
use crate::account_mesh::{
    ledger::LinkAdmission,
    transcript::{self, Party},
    AccountMeshAuthority, SignedMembership, VerifiedMembership,
};
use crate::{MeshManager, MeshState};
use codeterm_entitlements::DetachedGrant;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::WebSocketStream;

pub(super) const PATH: &str = "/cloud/renewal-recovery/v1";
const DOMAIN: &[u8] = b"domios-cloud-expired-renewal-v1";

#[derive(Clone, PartialEq, Eq, Serialize)]
struct Pin {
    party: Party,
    key_id: String,
    generation: u64,
    slot: u64,
}

impl Pin {
    fn new(
        mesh: &MeshManager,
        authority: &dyn AccountMeshAuthority,
        party: Party,
        grant: &SignedMembership,
    ) -> Result<Self, IngressError> {
        let member = &grant.verified;
        let state = mesh.account_state();
        if member.installation_id != party.installation_id
            || state.signed_in_account() != Some(member.account_id.as_str())
            || member.issued_at
                > mesh
                    .now()
                    .saturating_add(crate::account_mesh::NOT_BEFORE_SKEW_SECONDS)
            || state
                .revoked_installations
                .contains(&member.installation_id)
            || state
                .unavailable_installations
                .contains(&member.installation_id)
            || authority.verify_membership(&grant.token, member.issued_at)? != *member
        {
            return Err(IngressError::Rejected);
        }
        mesh.check_registry_membership(member)?;
        Ok(Self {
            party,
            key_id: member.key_id.clone(),
            generation: member.key_generation,
            slot: member.slot_revision,
        })
    }
}

struct Recovery {
    home: Pin,
    cloud: Pin,
    account: String,
    epoch: u64,
    access: u64,
    peer_epoch: u64,
    authority: Arc<dyn AccountMeshAuthority>,
}

impl Recovery {
    fn pinned(
        mesh: &MeshManager,
        other: &str,
        on_home: bool,
    ) -> Result<(Self, SignedMembership, SignedMembership), IngressError> {
        let state = mesh.account_state();
        if state.plan_limit_locked || mesh.inbound_only == on_home {
            return Err(IngressError::Rejected);
        }
        let authority = mesh.account_authority().ok_or(IngressError::Rejected)?;
        let own = state.membership.as_ref().ok_or(IngressError::Rejected)?;
        let identity = &mesh.ident.identity;
        let own_party = Party {
            instance_id: identity.instance_id.clone(),
            installation_id: own.verified.installation_id.clone(),
            mesh_public_key: identity.public_key.clone().ok_or(IngressError::Rejected)?,
        };
        let local = Pin::new(mesh, authority.as_ref(), own_party, own)?;
        let mut matches = mesh.paired_peers.values().filter_map(|peer| {
            let link = mesh.link_record(&peer.instance_id)?;
            let LinkAdmission::Account {
                account_id,
                installation_id,
                public_key,
            } = &link.admission;
            (installation_id == other
                && account_id == &own.verified.account_id
                && public_key == &peer.public_key)
                .then_some((peer, link))
        });
        let (paired, link) = matches.next().ok_or(IngressError::Rejected)?;
        if matches.next().is_some()
            || other == own.verified.installation_id
            || paired.instance_id == identity.instance_id
        {
            return Err(IngressError::Rejected);
        }
        let cached = link
            .remote_membership
            .as_ref()
            .ok_or(IngressError::Rejected)?;
        let peer = Pin::new(
            mesh,
            authority.as_ref(),
            Party {
                instance_id: paired.instance_id.clone(),
                mesh_public_key: paired.public_key.clone(),
                installation_id: other.into(),
            },
            cached,
        )?;
        let (home, cloud, old) = if on_home {
            if state.current_membership(mesh.now()).is_none() {
                return Err(IngressError::Rejected);
            }
            (local, peer, cached)
        } else {
            (peer, local, own)
        };
        if old.verified.expires_at > mesh.now() {
            return Err(IngressError::Rejected);
        }
        Ok((
            Self {
                home,
                cloud,
                account: own.verified.account_id.clone(),
                epoch: state.account_epoch,
                access: mesh.remote_access_epoch().ok_or(IngressError::Rejected)?,
                peer_epoch: mesh.peer_grant_epoch(&paired.instance_id),
                authority,
            },
            own.clone(),
            old.clone(),
        ))
    }

    fn fence(&self, mesh: &MeshManager, on_home: bool) -> Result<SignedMembership, IngressError> {
        let other = if on_home { &self.cloud } else { &self.home };
        let (fresh, own, _) = Self::pinned(mesh, &other.party.installation_id, on_home)?;
        if self.home != fresh.home
            || self.cloud != fresh.cloud
            || self.epoch != fresh.epoch
            || self.account != fresh.account
            || self.access != fresh.access
            || self.peer_epoch != fresh.peer_epoch
            || !Arc::ptr_eq(&self.authority, &fresh.authority)
        {
            return Err(IngressError::Rejected);
        }
        Ok(own)
    }

    fn digest(&self, binding: &[u8; 32]) -> Result<[u8; 32], IngressError> {
        let mut hash = Sha256::new();
        hash.update(DOMAIN);
        hash.update(binding);
        hash.update(
            serde_json::to_vec(&(&self.account, &self.home, &self.cloud))
                .map_err(|_| IngressError::InvalidFrame)?,
        );
        Ok(hash.finalize().into())
    }

    fn proof(
        &self,
        mesh: &MeshManager,
        on_home: bool,
        binding: &[u8; 32],
    ) -> Result<Proof, IngressError> {
        self.fence(mesh, on_home)?;
        let digest = self.digest(binding)?;
        let role = if on_home {
            transcript::INITIATOR_ROLE
        } else {
            transcript::RESPONDER_ROLE
        };
        let secret = mesh
            .ident
            .identity
            .private_key
            .as_deref()
            .ok_or(IngressError::Rejected)?;
        Ok(Proof {
            installation: self
                .authority
                .sign_possession(&transcript::possession_digest(&digest, role))?,
            mesh: transcript::sign_mesh(secret, &digest, role)
                .map_err(|_| IngressError::Rejected)?,
        })
    }

    fn verify(
        &self,
        pin: &Pin,
        role: &[u8],
        binding: &[u8; 32],
        proof: &Proof,
    ) -> Result<(), IngressError> {
        let digest = self.digest(binding)?;
        if proof.installation.len() > 2048
            || proof.mesh.len() > 128
            || !self.authority.verify_key_possession(
                &pin.key_id,
                &transcript::possession_digest(&digest, role),
                &proof.installation,
            )
            || !transcript::verify_mesh(&pin.party.mesh_public_key, &digest, role, &proof.mesh)
        {
            return Err(IngressError::Rejected);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    installation: String,
    mesh: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    home: String,
    proof: Proof,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Renewal {
        home_membership: String,
        membership: DetachedGrant,
        signed_registry: DetachedGrant,
    },
}

pub(super) async fn respond<S: ChannelIo>(
    socket: WebSocketStream<S>,
    mesh: &MeshState,
    receiver: &dyn RenewalReceiver,
    enabled: impl Fn() -> bool,
) -> Result<(), IngressError> {
    let mut stream = EncryptedStream::handshake_with_domain(socket, false, DOMAIN).await?;
    let hello: Hello = stream.read().await?;
    let (recovery, proof) = {
        let manager = mesh.lock();
        let (recovery, _, _) = Recovery::pinned(&manager, &hello.home, false)?;
        recovery.verify(
            &recovery.home,
            transcript::INITIATOR_ROLE,
            &stream.binding,
            &hello.proof,
        )?;
        let proof = recovery.proof(&manager, false, &stream.binding)?;
        (recovery, proof)
    };
    stream.write(&proof).await?;
    let Operation::Renewal {
        home_membership,
        membership,
        signed_registry,
    } = stream.read().await?;
    {
        let manager = mesh.lock();
        recovery.fence(&manager, false)?;
        if !receiver.enrolled() || !enabled() || home_membership.len() > 16384 {
            return Err(IngressError::Rejected);
        }
        let active = recovery
            .authority
            .verify_membership(&home_membership, manager.now())?;
        if !active.is_current(manager.now())
            || active.account_id != recovery.account
            || active.installation_id != recovery.home.party.installation_id
            || active.key_id != recovery.home.key_id
            || active.key_generation != recovery.home.generation
            || active.slot_revision != recovery.home.slot
        {
            return Err(IngressError::Rejected);
        }
    }
    receiver.apply(&membership, &signed_registry).await?;
    stream.write(&()).await
}

#[cfg(test)]
mod tests;

async fn initiate<S: ChannelIo>(
    stream: &mut EncryptedStream<S>,
    mesh: &MeshState,
    recovery: Recovery,
    old: &VerifiedMembership,
) -> Result<(), IngressError> {
    let proof = recovery.proof(&mesh.lock(), true, &stream.binding)?;
    stream
        .write(&Hello {
            home: recovery.home.party.installation_id.clone(),
            proof,
        })
        .await?;
    let proof = stream.read().await?;
    recovery.verify(
        &recovery.cloud,
        transcript::RESPONDER_ROLE,
        &stream.binding,
        &proof,
    )?;
    recovery.fence(&mesh.lock(), true)?;
    let (membership, signed_registry) = recovery.authority.renew_cloud(old).await?;
    let home_membership = recovery.fence(&mesh.lock(), true)?.token;
    stream
        .write(&Operation::Renewal {
            home_membership,
            membership,
            signed_registry,
        })
        .await?;
    stream.read::<()>().await
}

pub async fn recover_expired_cloud(
    mesh: &MeshState,
    route: CloudRoute,
    id: &str,
) -> Result<(), IngressError> {
    let (recovery, _, old) = Recovery::pinned(&mesh.lock(), id, true)?;
    tokio::time::timeout(Duration::from_secs(120), async {
        let socket = dial::open(route, PATH).await?;
        let mut stream = EncryptedStream::handshake_with_domain(socket, true, DOMAIN).await?;
        initiate(&mut stream, mesh, recovery, &old.verified).await
    })
    .await
    .map_err(|_| IngressError::Unavailable)?
}
