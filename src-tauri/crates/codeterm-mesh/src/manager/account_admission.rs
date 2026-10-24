use std::sync::Arc;

use super::MeshManager;
use crate::account_mesh::protocol::{
    AdoptCommit, AdoptCommitReply, AdoptOffer, AdoptOfferReply, LinkCommit, LinkConfirm, LinkHello,
    LinkHelloReply, PartyDescriptor,
};
use crate::account_mesh::transcript::{
    self, possession_digest, Party, Purpose, COMMIT_ROLE, INITIATOR_ROLE, RESPONDER_ROLE,
};
use crate::account_mesh::{
    AccountMeshAuthority, AdmissionRefusal, LinkAuthorizationRequest, LinkPermitExpectation,
    SignedMembership,
};
use crate::types::{self, PairedPeer};

pub(crate) const PENDING_TTL_SECS: u64 = 60;
pub(crate) const MAX_PENDING: usize = 8;

pub struct PendingLink {
    pub(super) initiator: PartyDescriptor,
    pub(super) initiator_membership: SignedMembership,
    pub(super) transcript: [u8; 32],
    pub(super) account_id: String,
    pub(super) account_epoch: u64,
    pub(super) created_at: u64,
    pub(super) approved: bool,
    removal_watermark: u64,
    /// Both permits are held; the pair is recorded only on the initiator's commit.
    confirmed: bool,
}

/// Only a local, user-initiated Add may lift this machine's own removal of a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOrigin {
    LocalAdd { removal_watermark: u64 },
    Remote,
}

pub struct PendingAdoption {
    peer_id: String,
    initiator_membership: SignedMembership,
    transcript: [u8; 32],
    account_id: String,
    account_epoch: u64,
    created_at: u64,
}

/// What the responder must hold after verifying a confirm, before its own online permit.
pub struct LinkConfirmation {
    pending: PendingLink,
    pub request: LinkAuthorizationRequest,
    pub authority: Arc<dyn AccountMeshAuthority>,
}

/// Local authority captured once so every later step can be fenced against it.
#[derive(Clone)]
pub struct AdmissionContext {
    pub authority: Arc<dyn AccountMeshAuthority>,
    pub account_id: String,
    pub account_epoch: u64,
    pub removal_watermark: u64,
    pub membership: SignedMembership,
    pub party: Party,
    pub private_key: String,
}

impl MeshManager {
    /// Signed-in, currently-enrolled local authority, or the refusal that forbids admission.
    pub fn admission_context(&self) -> Result<AdmissionContext, AdmissionRefusal> {
        let account_id = self
            .account
            .signed_in_account()
            .ok_or(AdmissionRefusal::SignedOut)?
            .to_string();
        let authority = self
            .account_authority
            .clone()
            .ok_or(AdmissionRefusal::Unauthenticated)?;
        let membership = self
            .account
            .current_membership(self.now())
            .cloned()
            .ok_or(AdmissionRefusal::Unauthenticated)?;
        let private_key = self
            .ident
            .identity
            .private_key
            .clone()
            .ok_or_else(|| AdmissionRefusal::handshake("missing local mesh key"))?;
        let mesh_public_key = match self.ident.identity.public_key.clone() {
            Some(key) => key,
            None => crate::crown::public_key_from_private(&private_key)
                .map_err(AdmissionRefusal::handshake)?,
        };
        Ok(AdmissionContext {
            party: Party {
                instance_id: self.ident.identity.instance_id.clone(),
                mesh_public_key,
                installation_id: membership.verified.installation_id.clone(),
            },
            authority,
            account_id,
            account_epoch: self.account.account_epoch,
            removal_watermark: self.removal_watermark(),
            membership,
            private_key,
        })
    }

    /// Fails unless the account that started an admission is still the signed-in one.
    pub fn fence_admission(&self, context: &AdmissionContext) -> Result<(), AdmissionRefusal> {
        if self.account.presence == crate::account_mesh::AccountPresence::SignedOut {
            return Err(AdmissionRefusal::SignedOut);
        }
        let same_account = self.account.signed_in_account() == Some(context.account_id.as_str());
        if self.account.account_epoch != context.account_epoch || !same_account {
            return Err(AdmissionRefusal::Cancelled);
        }
        Ok(())
    }

    pub fn local_descriptor(&self, party: Party) -> PartyDescriptor {
        PartyDescriptor {
            party,
            instance_name: self.ident.instance_name.clone(),
            api_port: self.api_port,
            addresses: self.local_announce_addresses(),
        }
    }

    fn verify_peer_membership(
        &self,
        context: &AdmissionContext,
        token: &str,
        installation_id: &str,
    ) -> Result<SignedMembership, AdmissionRefusal> {
        let verified = context.authority.verify_membership(token, self.now())?;
        if verified.account_id != context.account_id {
            return Err(AdmissionRefusal::handshake(
                "peer belongs to another account",
            ));
        }
        if verified.installation_id != installation_id || !verified.is_current(self.now()) {
            return Err(AdmissionRefusal::handshake(
                "peer membership does not match",
            ));
        }
        if verified.installation_id == context.party.installation_id {
            return Err(AdmissionRefusal::handshake("cannot link this installation"));
        }
        if self
            .account
            .revoked_installations
            .contains(&verified.installation_id)
        {
            return Err(AdmissionRefusal::Revoked);
        }
        Ok(SignedMembership {
            token: token.to_string(),
            verified,
        })
    }

    /// A new link may reuse an existing record only with the identical transport key.
    pub(crate) fn check_link_identity(
        &self,
        instance_id: &str,
        public_key: &str,
    ) -> Result<(), AdmissionRefusal> {
        uuid::Uuid::parse_str(instance_id)
            .map_err(|_| AdmissionRefusal::handshake("invalid peer identity"))?;
        if instance_id == self.ident.identity.instance_id {
            return Err(AdmissionRefusal::handshake("cannot link this instance"));
        }
        if self.is_blocked(instance_id) {
            return Err(AdmissionRefusal::handshake("peer identity is blocked"));
        }
        if let Some(existing) = self.paired_peers.get(instance_id) {
            if existing.public_key != public_key {
                return Err(AdmissionRefusal::handshake(
                    "peer key differs from the paired key",
                ));
            }
        }
        if crate::crown::verifying_key_from_b64(public_key).is_err() {
            return Err(AdmissionRefusal::handshake("invalid peer key"));
        }
        Ok(())
    }

    fn prune_pending(&mut self) {
        let now = self.now();
        self.pending_links
            .retain(|id, pending| {
                let ttl = if self.link_requests.contains_key(id) {
                    crate::account_mesh::approval::APPROVAL_TTL_SECS + PENDING_TTL_SECS
                } else { PENDING_TTL_SECS };
                now.saturating_sub(pending.created_at) <= ttl
            });
        self.pending_adoptions
            .retain(|_, pending| now.saturating_sub(pending.created_at) <= PENDING_TTL_SECS);
    }

    /// Responder, step 1: verify the initiator's membership, then commit to a fresh nonce.
    pub fn accept_link_hello(
        &mut self,
        hello: LinkHello,
    ) -> Result<LinkHelloReply, AdmissionRefusal> {
        let context = self.admission_context()?;
        let initiator = &hello.initiator.party;
        self.check_link_identity(&initiator.instance_id, &initiator.mesh_public_key)?;
        let membership =
            self.verify_peer_membership(&context, &hello.membership, &initiator.installation_id)?;
        self.prune_pending();
        if self.pending_links.len() >= MAX_PENDING {
            return Err(AdmissionRefusal::RateLimited {
                retry_after_seconds: PENDING_TTL_SECS,
            });
        }
        if self.pending_links.contains_key(&hello.link_id) {
            return Err(AdmissionRefusal::handshake("duplicate link id"));
        }
        if self.pending_links.iter().any(|(id, pending)| pending.initiator.party.installation_id == initiator.installation_id && self.link_requests.get(id).is_some_and(|request| request.decision_at(self.now()) == crate::account_mesh::approval::LinkDecision::Waiting)) {
            return Err(AdmissionRefusal::handshake("link request already pending"));
        }
        let superseded: Vec<String> = self.pending_links.iter().filter(|(_, pending)| pending.initiator.party.installation_id == initiator.installation_id).map(|(id, _)| id.clone()).collect();
        for id in superseded { self.clear_link_request(&id); }
        let nonce = transcript::fresh_nonce();
        let digest = transcript::transcript(
            Purpose::Link,
            initiator,
            &context.party,
            &context.account_id,
            &hello.nonce,
            &nonce,
        );
        let installation_signature = context
            .authority
            .sign_possession(&possession_digest(&digest, RESPONDER_ROLE))?;
        let mesh_signature = transcript::sign_mesh(&context.private_key, &digest, RESPONDER_ROLE)
            .map_err(AdmissionRefusal::handshake)?;
        self.pending_links.insert(
            hello.link_id.clone(),
            PendingLink {
                initiator: hello.initiator,
                initiator_membership: membership,
                transcript: digest,
                account_id: context.account_id.clone(),
                account_epoch: context.account_epoch,
                created_at: self.now(),
                approved: false,
                removal_watermark: context.removal_watermark,
                confirmed: false,
            },
        );
        Ok(LinkHelloReply {
            link_id: hello.link_id,
            responder: self.local_descriptor(context.party),
            membership: context.membership.token,
            nonce,
            installation_signature,
            mesh_signature,
        })
    }

    /// Responder, step 2a: consume the pending entry and verify the initiator's proof and permit.
    pub fn begin_link_confirm(
        &mut self,
        confirm: &LinkConfirm,
    ) -> Result<LinkConfirmation, AdmissionRefusal> {
        self.prune_pending();
        if !self.pending_links.get(&confirm.link_id).is_some_and(|pending| pending.approved) {
            return Err(AdmissionRefusal::handshake("link requires local approval"));
        }
        if self
            .pending_links
            .get(&confirm.link_id)
            .is_some_and(|pending| pending.confirmed)
        {
            return Err(AdmissionRefusal::handshake("link already confirmed"));
        }
        let pending = self
            .pending_links
            .remove(&confirm.link_id)
            .ok_or_else(|| AdmissionRefusal::handshake("unknown or expired link"))?;
        let context = self.admission_context()?;
        if context.account_epoch != pending.account_epoch
            || context.account_id != pending.account_id
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        let initiator = &pending.initiator.party;
        if !transcript::verify_mesh(
            &initiator.mesh_public_key,
            &pending.transcript,
            INITIATOR_ROLE,
            &confirm.mesh_signature,
        ) {
            return Err(AdmissionRefusal::handshake("initiator mesh proof invalid"));
        }
        if !context.authority.verify_possession(
            &pending.initiator_membership.verified,
            &possession_digest(&pending.transcript, INITIATOR_ROLE),
            &confirm.installation_signature,
        ) {
            return Err(AdmissionRefusal::handshake(
                "initiator installation proof invalid",
            ));
        }
        context.authority.verify_link_permit(
            &confirm.permit,
            &LinkPermitExpectation {
                transcript: pending.transcript,
                holder_installation_id: initiator.installation_id.clone(),
                peer_installation_id: context.party.installation_id.clone(),
                account_id: context.account_id.clone(),
            },
            self.now(),
        )?;
        let request = LinkAuthorizationRequest {
            transcript: pending.transcript,
            local_installation_id: context.party.installation_id.clone(),
            peer_installation_id: initiator.installation_id.clone(),
            account_id: context.account_id.clone(),
            link_nonce: confirm.link_id.clone(),
        };
        Ok(LinkConfirmation {
            pending,
            request,
            authority: context.authority,
        })
    }

    /// Responder, step 2b: both permits are held. Nothing is recorded or traffic-open until
    /// the initiator commits; an uncommitted link expires with the pending window.
    pub fn finish_link_confirm(
        &mut self,
        confirmation: LinkConfirmation,
    ) -> Result<(), AdmissionRefusal> {
        let mut pending = confirmation.pending;
        if self.account.account_epoch != pending.account_epoch
            || self.account.signed_in_account() != Some(pending.account_id.as_str())
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        pending.confirmed = true;
        pending.created_at = self.now();
        self.pending_links
            .insert(confirmation.request.link_nonce.clone(), pending);
        Ok(())
    }

    /// Responder, step 3: the initiator proved it verified our permit; record the pair.
    pub fn commit_link(&mut self, commit: &LinkCommit) -> Result<(), AdmissionRefusal> {
        self.prune_pending();
        if !self
            .pending_links
            .get(&commit.link_id)
            .is_some_and(|pending| pending.confirmed)
        {
            return Err(AdmissionRefusal::handshake("unknown or unconfirmed link"));
        }
        self.link_requests.remove(&commit.link_id);
        let pending = self
            .pending_links
            .remove(&commit.link_id)
            .ok_or_else(|| AdmissionRefusal::handshake("unknown or unconfirmed link"))?;
        if self.account.account_epoch != pending.account_epoch
            || self.account.signed_in_account() != Some(pending.account_id.as_str())
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        if !transcript::verify_mesh(
            &pending.initiator.party.mesh_public_key,
            &pending.transcript,
            COMMIT_ROLE,
            &commit.mesh_signature,
        ) {
            return Err(AdmissionRefusal::handshake(
                "initiator commit proof invalid",
            ));
        }
        self.admit_new_link(
            &pending.initiator,
            &[],
            pending.initiator_membership,
            &pending.account_id,
            LinkOrigin::LocalAdd { removal_watermark: pending.removal_watermark },
            &commit.link_id,
        )
    }

    fn refuse_removed_peer(&self, instance_id: &str) -> Result<(), AdmissionRefusal> {
        if self.is_revoked(instance_id) {
            return Err(AdmissionRefusal::handshake(
                "peer was removed on this machine; add it from here to link again",
            ));
        }
        Ok(())
    }

    /// Persists an account-admitted edge. Existing records keep their access policy.
    pub fn admit_new_link(
        &mut self,
        peer: &PartyDescriptor,
        preferred_addresses: &[types::PeerAddress],
        membership: SignedMembership,
        account_id: &str,
        origin: LinkOrigin,
        link_nonce: &str,
    ) -> Result<(), AdmissionRefusal> {
        let party = &peer.party;
        self.check_link_identity(&party.instance_id, &party.mesh_public_key)?;
        match origin {
            LinkOrigin::Remote => self.refuse_removed_peer(&party.instance_id)?,
            LinkOrigin::LocalAdd { removal_watermark } => {
                if self
                    .revocation_list
                    .tombstones
                    .get(&party.instance_id)
                    .is_some_and(|removed_at| *removed_at > removal_watermark)
                {
                    return Err(AdmissionRefusal::Cancelled);
                }
            }
        }
        let mut addresses = types::sanitize_addresses(preferred_addresses);
        for address in types::sanitize_addresses(&peer.addresses) {
            if !addresses.iter().any(|known| known.ip == address.ip) {
                addresses.push(address);
            }
        }
        self.links
            .admit(
                &party.instance_id,
                account_id,
                &party.installation_id,
                &party.mesh_public_key,
                membership,
            )
            .map_err(AdmissionRefusal::handshake)?;
        if let Some(record) = self.links.links.get_mut(&party.instance_id) { record.link_nonce = Some(link_nonce.into()); }
        self.links.disconnects.remove(&party.instance_id);
        let record = self
            .paired_peers
            .entry(party.instance_id.clone())
            .or_insert_with(|| PairedPeer {
                instance_id: party.instance_id.clone(),
                instance_name: peer.instance_name.clone(),
                addresses: Vec::new(),
                api_port: peer.api_port,
                public_key: party.mesh_public_key.clone(),
                paired_at: chrono::Utc::now().to_rfc3339(),
                last_seen: None,
                online: true,
                is_primary: false,
                access_level: types::AccessLevel::default(),
                version: None,
                web_ui: None,
                relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
                consecutive_misses: 0,
                last_active_ms: None,
            });
        if !addresses.is_empty() {
            record.addresses = addresses;
            record.api_port = peer.api_port;
        }
        if matches!(origin, LinkOrigin::LocalAdd { .. }) {
            self.revocation_list
                .revoked
                .retain(|id| id != &party.instance_id);
            self.revocation_list.tombstones.remove(&party.instance_id);
        }
        self.save().map_err(AdmissionRefusal::handshake)?;
        self.bump_gate();
        log::info!("[mesh/account] linked {}", party.instance_id);
        Ok(())
    }

    /// Adoption responder, step 1: only an open legacy (or same-account adopted) edge qualifies.
    pub fn accept_adopt_offer(
        &mut self,
        peer_id: &str,
        offer: AdoptOffer,
    ) -> Result<AdoptOfferReply, AdmissionRefusal> {
        let context = self.admission_context()?;
        self.check_adoptable(peer_id, &context)?;
        let peer_key = self
            .paired_peers
            .get(peer_id)
            .map(|peer| peer.public_key.clone())
            .ok_or_else(|| AdmissionRefusal::handshake("peer is not paired"))?;
        let membership =
            self.verify_peer_membership(&context, &offer.membership, &offer.installation_id)?;
        self.prune_pending();
        if self.pending_adoptions.len() >= MAX_PENDING {
            return Err(AdmissionRefusal::RateLimited {
                retry_after_seconds: PENDING_TTL_SECS,
            });
        }
        let initiator = Party {
            instance_id: peer_id.to_string(),
            mesh_public_key: peer_key,
            installation_id: offer.installation_id.clone(),
        };
        let nonce = transcript::fresh_nonce();
        let digest = transcript::transcript(
            Purpose::Adopt,
            &initiator,
            &context.party,
            &context.account_id,
            &offer.nonce,
            &nonce,
        );
        let installation_signature = context
            .authority
            .sign_possession(&possession_digest(&digest, RESPONDER_ROLE))?;
        let mesh_signature = transcript::sign_mesh(&context.private_key, &digest, RESPONDER_ROLE)
            .map_err(AdmissionRefusal::handshake)?;
        self.pending_adoptions.insert(
            offer.adoption_id.clone(),
            PendingAdoption {
                peer_id: peer_id.to_string(),
                initiator_membership: membership,
                transcript: digest,
                account_id: context.account_id.clone(),
                account_epoch: context.account_epoch,
                created_at: self.now(),
            },
        );
        Ok(AdoptOfferReply {
            adoption_id: offer.adoption_id,
            installation_id: context.party.installation_id.clone(),
            membership: context.membership.token,
            nonce,
            installation_signature,
            mesh_signature,
        })
    }

    /// Adoption responder, step 2: verify the initiator's proof, then adopt in place.
    pub fn commit_adoption(
        &mut self,
        peer_id: &str,
        commit: AdoptCommit,
    ) -> Result<AdoptCommitReply, AdmissionRefusal> {
        self.prune_pending();
        let pending = self
            .pending_adoptions
            .remove(&commit.adoption_id)
            .filter(|pending| pending.peer_id == peer_id)
            .ok_or_else(|| AdmissionRefusal::handshake("unknown or expired adoption"))?;
        let context = self.admission_context()?;
        if context.account_epoch != pending.account_epoch
            || context.account_id != pending.account_id
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        let peer_key = self
            .paired_peers
            .get(peer_id)
            .map(|peer| peer.public_key.clone())
            .ok_or_else(|| AdmissionRefusal::handshake("peer is not paired"))?;
        if !transcript::verify_mesh(
            &peer_key,
            &pending.transcript,
            INITIATOR_ROLE,
            &commit.mesh_signature,
        ) || !context.authority.verify_possession(
            &pending.initiator_membership.verified,
            &possession_digest(&pending.transcript, INITIATOR_ROLE),
            &commit.installation_signature,
        ) {
            return Err(AdmissionRefusal::handshake(
                "initiator adoption proof invalid",
            ));
        }
        let installation_id = pending
            .initiator_membership
            .verified
            .installation_id
            .clone();
        self.commit_local_adoption(
            peer_id,
            &context,
            &installation_id,
            pending.initiator_membership,
        )?;
        Ok(AdoptCommitReply { adopted: true })
    }

    pub(crate) fn check_adoptable(
        &self,
        peer_id: &str,
        context: &AdmissionContext,
    ) -> Result<(), AdmissionRefusal> {
        if self
            .owner_account()
            .is_some_and(|owner| owner != context.account_id)
        {
            return Err(AdmissionRefusal::handshake(
                "legacy edge belongs to another account",
            ));
        }
        match self.links.get(peer_id).map(|record| &record.admission) {
            Some(crate::account_mesh::ledger::LinkAdmission::Legacy { .. }) => {
                if self.traffic_open(peer_id) {
                    Ok(())
                } else {
                    Err(AdmissionRefusal::handshake("legacy edge is not open"))
                }
            }
            Some(crate::account_mesh::ledger::LinkAdmission::Adopted { account_id, .. })
                if account_id == &context.account_id =>
            {
                Ok(())
            }
            _ => Err(AdmissionRefusal::handshake(
                "edge has no adoptable provenance",
            )),
        }
    }

    /// Monotonic in-place adoption: keys, record and access policy stay as they are.
    pub fn commit_local_adoption(
        &mut self,
        peer_id: &str,
        context: &AdmissionContext,
        installation_id: &str,
        remote: SignedMembership,
    ) -> Result<(), AdmissionRefusal> {
        self.fence_admission(context)?;
        let peer_key = self
            .paired_peers
            .get(peer_id)
            .map(|peer| peer.public_key.clone())
            .ok_or_else(|| AdmissionRefusal::handshake("peer is not paired"))?;
        self.links
            .admit(
                peer_id,
                &context.account_id,
                installation_id,
                &peer_key,
                remote,
            )
            .map_err(AdmissionRefusal::handshake)?;
        self.save().map_err(AdmissionRefusal::handshake)?;
        self.bump_gate();
        log::info!("[mesh/account] adopted {}", peer_id);
        Ok(())
    }
}
