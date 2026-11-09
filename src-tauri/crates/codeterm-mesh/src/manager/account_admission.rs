use std::sync::Arc;

use super::{ConnectionPhase, MeshManager};
use crate::account_mesh::protocol::{
    LinkCommit, LinkConfirm, LinkHello, LinkHelloReply, PartyDescriptor,
};
use crate::account_mesh::transcript::{
    self, possession_digest, Party, Purpose, COMMIT_ROLE, INITIATOR_ROLE, RESPONDER_ROLE,
};
use crate::account_mesh::{AccountMeshAuthority, AdmissionRefusal, SignedMembership};
use crate::types::{self, PairedPeer};

pub(crate) const PENDING_TTL_SECS: u64 = 60;
pub(crate) const MAX_PENDING: usize = 8;

#[derive(Clone)]
pub struct PendingLink {
    pub(super) initiator: PartyDescriptor,
    pub(super) initiator_membership: SignedMembership,
    pub(super) transcript: [u8; 32],
    pub(super) account_id: String,
    pub(super) account_epoch: u64,
    pub(super) created_at: u64,
    confirmed: bool,
    confirming: bool,
}

/// A reserved confirmation fenced against the account epoch.
pub struct LinkConfirmation {
    pending: PendingLink,
    link_id: String,
}

/// Local authority captured once so every later step can be fenced against it.
#[derive(Clone)]
pub struct AdmissionContext {
    pub authority: Arc<dyn AccountMeshAuthority>,
    pub account_id: String,
    pub account_epoch: u64,
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
        self.check_registry_membership(&membership.verified)?;
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
        self.check_registry_membership(&verified)?;
        Ok(SignedMembership {
            token: token.to_string(),
            verified,
        })
    }

    pub(crate) fn check_registry_membership(
        &self,
        membership: &crate::account_mesh::VerifiedMembership,
    ) -> Result<(), AdmissionRefusal> {
        let matches = self
            .account
            .registry_members
            .as_ref()
            .and_then(|members| members.get(&membership.installation_id))
            .is_some_and(|(key, generation, slot)| {
                key == &membership.key_id
                    && *generation == membership.key_generation
                    && *slot == membership.slot_revision
            });
        if matches {
            Ok(())
        } else {
            Err(AdmissionRefusal::Revoked)
        }
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

    pub fn connection_deadline(&self) -> Option<u64> {
        self.pending_links
            .values()
            .map(|pending| pending.created_at.saturating_add(PENDING_TTL_SECS + 1))
            .chain(
                self.paired_peers
                    .keys()
                    .filter_map(|peer| self.admission_deadline(peer))
                    .filter(|deadline| *deadline > self.now()),
            )
            .chain(
                self.peer_request_retry
                    .values()
                    .copied()
                    .filter(|deadline| *deadline > self.now()),
            )
            .min()
    }

    pub fn expire_pending_connections(&mut self) {
        self.prune_pending();
    }

    pub fn fail_pending_connection(&mut self, link_id: &str) {
        if let Some(pending) = self.pending_links.remove(link_id) {
            self.set_connection_phase(
                &pending.initiator.party.installation_id,
                ConnectionPhase::Failed,
            );
        }
    }

    fn prune_pending(&mut self) {
        let now = self.now();
        let expired: Vec<_> = self
            .pending_links
            .iter()
            .filter(|(_, pending)| now.saturating_sub(pending.created_at) > PENDING_TTL_SECS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.fail_pending_connection(&id);
        }
    }

    /// Responder, step 1: verify the initiator's membership, then commit to a fresh nonce.
    pub fn accept_link_hello(
        &mut self,
        hello: LinkHello,
    ) -> Result<LinkHelloReply, AdmissionRefusal> {
        if hello.protocol_version != crate::account_mesh::protocol::LINK_PROTOCOL_VERSION {
            return Err(AdmissionRefusal::PeerNeedsUpdate);
        }
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
        self.set_connection_phase(
            &hello.initiator.party.installation_id,
            ConnectionPhase::Verifying,
        );
        self.pending_links.insert(
            hello.link_id.clone(),
            PendingLink {
                initiator: hello.initiator,
                initiator_membership: membership,
                transcript: digest,
                account_id: context.account_id.clone(),
                account_epoch: context.account_epoch,
                created_at: self.now(),
                confirmed: false,
                confirming: false,
            },
        );
        Ok(LinkHelloReply {
            protocol_version: crate::account_mesh::protocol::LINK_PROTOCOL_VERSION,
            signed_registry: self.signed_registry.clone(),
            link_id: hello.link_id,
            responder: self.local_descriptor(context.party),
            membership: context.membership.token,
            nonce,
            installation_signature,
            mesh_signature,
        })
    }

    /// Reserves the pending entry and verifies the initiator's possession.
    pub fn begin_link_confirm(
        &mut self,
        confirm: &LinkConfirm,
    ) -> Result<LinkConfirmation, AdmissionRefusal> {
        let result = self.begin_link_confirm_inner(confirm);
        if result.is_err() {
            self.fail_pending_connection(&confirm.link_id);
        }
        result
    }

    fn begin_link_confirm_inner(
        &mut self,
        confirm: &LinkConfirm,
    ) -> Result<LinkConfirmation, AdmissionRefusal> {
        self.prune_pending();
        if self
            .pending_links
            .get(&confirm.link_id)
            .is_some_and(|pending| pending.confirmed || pending.confirming)
        {
            return Err(AdmissionRefusal::handshake("link already confirmed"));
        }
        let pending = self
            .pending_links
            .get(&confirm.link_id)
            .cloned()
            .ok_or_else(|| AdmissionRefusal::handshake("unknown or expired link"))?;
        if let Some(entry) = self.pending_links.get_mut(&confirm.link_id) {
            entry.confirming = true;
        }
        let context = self.admission_context()?;
        if context.account_epoch != pending.account_epoch
            || context.account_id != pending.account_id
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        let initiator = &pending.initiator.party;
        self.verify_peer_membership(
            &context,
            &pending.initiator_membership.token,
            &initiator.installation_id,
        )?;
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
        Ok(LinkConfirmation {
            pending,
            link_id: confirm.link_id.clone(),
        })
    }

    /// Confirmation remains pending until the initiator commits.
    pub fn finish_link_confirm(
        &mut self,
        confirmation: LinkConfirmation,
    ) -> Result<(), AdmissionRefusal> {
        let mut pending = confirmation.pending;
        if !self.pending_links.contains_key(&confirmation.link_id)
            || self.now().saturating_sub(pending.created_at) > PENDING_TTL_SECS
        {
            self.fail_pending_connection(&confirmation.link_id);
            return Err(AdmissionRefusal::Cancelled);
        }
        if self.account.account_epoch != pending.account_epoch
            || self.account.signed_in_account() != Some(pending.account_id.as_str())
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        self.set_connection_phase(
            &pending.initiator.party.installation_id,
            ConnectionPhase::Committing,
        );
        pending.confirmed = true;
        pending.created_at = self.now();
        self.pending_links
            .insert(confirmation.link_id.clone(), pending);
        Ok(())
    }

    /// Records the pair after verifying the initiator's commit proof.
    pub fn commit_link(&mut self, commit: &LinkCommit) -> Result<(), AdmissionRefusal> {
        let result = self.commit_link_inner(commit);
        if result.is_err() {
            self.fail_pending_connection(&commit.link_id);
        }
        result
    }

    fn commit_link_inner(&mut self, commit: &LinkCommit) -> Result<(), AdmissionRefusal> {
        self.prune_pending();
        if !self
            .pending_links
            .get(&commit.link_id)
            .is_some_and(|pending| pending.confirmed)
        {
            return Err(AdmissionRefusal::handshake("unknown or unconfirmed link"));
        }
        let pending = self
            .pending_links
            .get(&commit.link_id)
            .cloned()
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
        self.pending_links.remove(&commit.link_id);
        let result = self.admit_new_link(
            &pending.initiator,
            &[],
            pending.initiator_membership,
            &pending.account_id,
            &commit.link_id,
        );
        self.set_connection_phase(
            &pending.initiator.party.installation_id,
            if result.is_ok() {
                ConnectionPhase::Ready
            } else {
                ConnectionPhase::Failed
            },
        );
        result
    }

    /// Persists an account-admitted edge. Existing records keep their access policy.
    pub fn admit_new_link(
        &mut self,
        peer: &PartyDescriptor,
        preferred_addresses: &[types::PeerAddress],
        membership: SignedMembership,
        account_id: &str,
        link_nonce: &str,
    ) -> Result<(), AdmissionRefusal> {
        let party = &peer.party;
        self.check_link_identity(&party.instance_id, &party.mesh_public_key)?;
        let context = self.admission_context()?;
        let membership =
            self.verify_peer_membership(&context, &membership.token, &party.installation_id)?;
        let addresses = types::sanitize_addresses(preferred_addresses);
        let superseded: Vec<String> = self
            .links
            .links
            .iter()
            .filter(|(id, record)| {
                id.as_str() != party.instance_id
                    && record.installation_id() == Some(party.installation_id.as_str())
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in superseded {
            self.remove_account_peer(&id);
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
        if let Some(record) = self.links.links.get_mut(&party.instance_id) {
            record.link_nonce = Some(link_nonce.into());
        }
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
        record.online = true;
        record.consecutive_misses = 0;
        record.instance_name = peer.instance_name.clone();
        self.save().map_err(AdmissionRefusal::handshake)?;
        self.peer_request_retry.remove(&party.instance_id);
        self.bump_gate();
        log::info!("[mesh/account] linked {}", party.instance_id);
        Ok(())
    }
}
