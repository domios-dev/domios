use super::MeshManager;
use crate::account_mesh::approval::{LinkDecision, LinkRequest, APPROVAL_TTL_SECS};
use crate::account_mesh::protocol::LinkApproval;
use crate::account_mesh::transcript::{self, possession_digest, INITIATOR_ROLE};
use crate::account_mesh::AdmissionRefusal;

impl MeshManager {
    pub fn request_link_approval(&mut self, proof: &LinkApproval) -> Result<LinkRequest, AdmissionRefusal> {
        let context = self.admission_context()?;
        let pending = self.pending_links.get(&proof.link_id)
            .ok_or_else(|| AdmissionRefusal::handshake("unknown link"))?;
        if self.now().saturating_sub(pending.created_at) > super::account_admission::PENDING_TTL_SECS {
            return Err(AdmissionRefusal::Expired);
        }
        if context.account_epoch != pending.account_epoch || context.account_id != pending.account_id {
            return Err(AdmissionRefusal::Cancelled);
        }
        if !transcript::verify_mesh(&pending.initiator.party.mesh_public_key, &pending.transcript, INITIATOR_ROLE, &proof.mesh_signature)
            || !context.authority.verify_possession(&pending.initiator_membership.verified, &possession_digest(&pending.transcript, INITIATOR_ROLE), &proof.installation_signature) {
            return Err(AdmissionRefusal::handshake("initiator approval proof invalid"));
        }
        if self.link_requests.contains_key(&proof.link_id) {
            return Err(AdmissionRefusal::handshake("approval already requested"));
        }
        let request = LinkRequest {
            notification_id: None,
            failure: None,
            request_id: proof.link_id.clone(),
            installation_id: pending.initiator.party.installation_id.clone(),
            name: pending.initiator.instance_name.clone(),
            incoming: true,
            expires_at: self.now().saturating_add(APPROVAL_TTL_SECS),
            decision: LinkDecision::Waiting,
        };
        self.link_requests.insert(proof.link_id.clone(), request.clone());
        self.bump_gate();
        Ok(request)
    }

    pub fn link_requests(&self) -> Vec<LinkRequest> {
        self.link_requests.values().map(|request| LinkRequest {
            decision: request.decision_at(self.now()),
            ..request.clone()
        }).collect()
    }

    pub fn decide_link(&mut self, request_id: &str, approve: bool) -> Result<LinkDecision, AdmissionRefusal> {
        self.admission_context()?;
        let now = self.now();
        let request = self.link_requests.get_mut(request_id)
            .filter(|request| request.incoming)
            .ok_or(AdmissionRefusal::Cancelled)?;
        let standing = request.decision_at(now);
        if standing != LinkDecision::Waiting {
            return match standing {
                LinkDecision::Expired => Err(AdmissionRefusal::Expired),
                _ => Ok(standing),
            };
        }
        request.decision = if approve { LinkDecision::Approved } else { LinkDecision::Declined };
        let decision = request.decision;
        if approve {
            let pending = self.pending_links.get_mut(request_id).ok_or(AdmissionRefusal::Cancelled)?;
            pending.approved = true;
            pending.created_at = now;
        }
        self.bump_gate();
        Ok(decision)
    }

    pub fn link_approval_result(&self, request_id: &str) -> Result<bool, AdmissionRefusal> {
        self.admission_context()?;
        let request = self.link_requests.get(request_id).ok_or(AdmissionRefusal::Cancelled)?;
        match request.decision_at(self.now()) {
            LinkDecision::Waiting => Ok(false),
            LinkDecision::Approved => Ok(true),
            LinkDecision::Declined => Err(AdmissionRefusal::Declined),
            LinkDecision::Expired => Err(AdmissionRefusal::Expired),
            LinkDecision::Failed => Err(AdmissionRefusal::Cancelled),
        }
    }

    pub fn expire_link_request(&mut self, request_id: &str) {
        if let Some(request) = self.link_requests.get_mut(request_id) {
            if request.decision == LinkDecision::Waiting {
                request.decision = LinkDecision::Expired;
                self.pending_links.remove(request_id);
                self.bump_gate();
            }
        }
    }

    pub fn clear_link_request(&mut self, request_id: &str) {
        self.link_requests.remove(request_id);
        self.pending_links.remove(request_id);
        self.bump_gate();
    }

    pub fn begin_outgoing_link(&mut self, installation_id: &str) -> Result<String, AdmissionRefusal> {
        self.admission_context()?;
        if self.link_requests.values().filter(|request| !request.incoming && request.decision_at(self.now()) == LinkDecision::Waiting).count() >= super::account_admission::MAX_PENDING {
            return Err(AdmissionRefusal::RateLimited { retry_after_seconds: APPROVAL_TTL_SECS });
        }
        if self.link_requests.len() >= 32 {
            let now = self.now();
            self.link_requests.retain(|_, request| request.incoming || request.decision_at(now) == LinkDecision::Waiting);
        }
        if self.link_requests.values().any(|request| !request.incoming && request.installation_id == installation_id && request.decision_at(self.now()) == LinkDecision::Waiting) {
            return Err(AdmissionRefusal::handshake("link request already pending"));
        }
        self.link_requests.retain(|_, request| request.incoming || request.installation_id != installation_id);
        let request_id = uuid::Uuid::new_v4().to_string();
        self.link_requests.insert(request_id.clone(), LinkRequest {
            notification_id: None, failure: None,
            request_id: request_id.clone(), installation_id: installation_id.into(), name: String::new(), incoming: false,
            expires_at: self.now().saturating_add(APPROVAL_TTL_SECS), decision: LinkDecision::Waiting,
        });
        self.bump_gate();
        Ok(request_id)
    }

    pub fn finish_outgoing_link(&mut self, request_id: &str, result: &Result<crate::account_mesh::protocol::AccountMeshPair, AdmissionRefusal>) {
        let decision = match result {
            Err(AdmissionRefusal::Declined) => Some(LinkDecision::Declined),
            Err(AdmissionRefusal::Expired) => Some(LinkDecision::Expired),
            Err(_) => Some(LinkDecision::Failed),
            Ok(_) => None,
        };
        if let Some(decision) = decision {
            if let Some(request) = self.link_requests.get_mut(request_id) { request.decision = decision; request.failure = result.as_ref().err().cloned(); }
        } else {
            self.link_requests.remove(request_id);
        }
        self.bump_gate();
    }

    pub fn set_link_notification(&mut self, request_id: &str, notification_id: String) {
        if let Some(request) = self.link_requests.get_mut(request_id) { request.notification_id = Some(notification_id); }
        self.bump_gate();
    }
}
