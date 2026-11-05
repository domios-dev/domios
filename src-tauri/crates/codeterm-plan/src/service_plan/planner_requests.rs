use super::store_db::hot_write::runtime::PlannerRows;
use crate::plan::{
    PlannerDeliveryPhase, PlannerDeliveryTransport, PlannerInboxEntry, PlannerRef, PlannerRequest,
    PlannerRequestState,
};

use super::{PlanError, PlanStore};

const DELIVERY_LEASE_MS: u64 = 30_000;

impl PlanStore {
    pub fn rebind_planner_session(
        &self,
        plan_id: &str,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let Some(PlannerRef::Tab {
                tab_id: bound,
                title,
                ..
            }) = &plan.planner
            else {
                return Ok(false);
            };
            if bound != tab_id {
                return Ok(false);
            }
            plan.planner = Some(PlannerRef::Tab {
                tab_id: tab_id.to_string(),
                session_id: Some(session_id.to_string()),
                title: title.clone(),
            });
            for request in &mut plan.planner_requests {
                if request.state != PlannerRequestState::InProgress
                    || request.delivered_to_tab_id.as_deref() != Some(tab_id)
                {
                    continue;
                }
                let Some(inbox) = plan
                    .planner_inbox
                    .iter_mut()
                    .find(|entry| entry.request_id == request.request_id)
                else {
                    continue;
                };
                if inbox.phase != PlannerDeliveryPhase::PreSend {
                    continue;
                }
                request.delivered_to_session_id = Some(session_id.to_string());
                request.delivered_with_binding_nonce = Some(binding_nonce.to_string());
                inbox.session_id = Some(session_id.to_string());
                inbox.binding_nonce = Some(binding_nonce.to_string());
                inbox.updated_at = now_ms;
            }
            Ok(true)
        })
    }

    pub fn queue_planner_request(
        &self,
        plan_id: &str,
        request_id: String,
        payload: String,
        caller_tab_id: Option<&str>,
        caller_session_id: Option<&str>,
        now_ms: u64,
    ) -> Result<PlannerRequest, PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            if let Some(existing) = plan
                .planner_requests
                .iter()
                .find(|request| request.request_id == request_id)
            {
                return Ok(existing.clone());
            }
            let request = PlannerRequest {
                request_id,
                payload,
                payload_blob: None,
                caller_tab_id: caller_tab_id.map(str::to_string),
                caller_session_id: caller_session_id.map(str::to_string),
                state: PlannerRequestState::Queued,
                created_at: now_ms,
                accepted_at: None,
                lease_expires_at: None,
                delivery_attempts: 0,
                completed_at: None,
                failed_at: None,
                delivered_to_tab_id: None,
                delivered_to_session_id: None,
                delivered_with_binding_nonce: None,
            };
            plan.planner_requests.push(request.clone());
            plan.planner_inbox.push(PlannerInboxEntry {
                request_id: request.request_id.clone(),
                phase: PlannerDeliveryPhase::PreSend,
                tab_id: None,
                session_id: None,
                binding_nonce: None,
                transport: None,
                last_error: None,
                updated_at: now_ms,
            });
            Ok(request)
        })
    }

    pub fn claim_planner_requests(
        &self,
        plan_id: &str,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
        now_ms: u64,
    ) -> Result<Vec<PlannerRequest>, PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            for request in &plan.planner_requests {
                if !plan
                    .planner_inbox
                    .iter()
                    .any(|entry| entry.request_id == request.request_id)
                {
                    plan.planner_inbox.push(PlannerInboxEntry {
                        request_id: request.request_id.clone(),
                        phase: PlannerDeliveryPhase::PreSend,
                        tab_id: None,
                        session_id: None,
                        binding_nonce: None,
                        transport: None,
                        last_error: None,
                        updated_at: now_ms,
                    });
                }
            }
            let mut claimed = Vec::new();
            for request in &mut plan.planner_requests {
                let inbox = plan
                    .planner_inbox
                    .iter_mut()
                    .find(|entry| entry.request_id == request.request_id)
                    .expect("planner request must have an inbox entry");
                let expired_lease = request.state == PlannerRequestState::InProgress
                    && request
                        .lease_expires_at
                        .is_some_and(|expires| expires <= now_ms);
                let replacement_pre_send = request.state == PlannerRequestState::InProgress
                    && inbox.phase == PlannerDeliveryPhase::PreSend
                    && (request.delivered_to_session_id.as_deref() != Some(session_id)
                        || request.delivered_with_binding_nonce.as_deref() != Some(binding_nonce));
                if expired_lease && inbox.phase == PlannerDeliveryPhase::SendStarted {
                    inbox.phase = PlannerDeliveryPhase::DeliveryUncertain;
                    inbox.last_error = Some("delivery lease expired after send started".into());
                    inbox.updated_at = now_ms;
                }
                if !matches!(
                    request.state,
                    PlannerRequestState::Queued | PlannerRequestState::Failed
                ) && !(expired_lease && inbox.phase == PlannerDeliveryPhase::PreSend)
                    && !replacement_pre_send
                {
                    continue;
                }
                if inbox.phase != PlannerDeliveryPhase::PreSend {
                    continue;
                }
                request.state = PlannerRequestState::InProgress;
                request.accepted_at = None;
                request.lease_expires_at = Some(now_ms.saturating_add(DELIVERY_LEASE_MS));
                request.delivery_attempts = request.delivery_attempts.saturating_add(1);
                request.delivered_to_tab_id = Some(tab_id.to_string());
                request.delivered_to_session_id = Some(session_id.to_string());
                request.delivered_with_binding_nonce = Some(binding_nonce.to_string());
                inbox.tab_id = Some(tab_id.to_string());
                inbox.session_id = Some(session_id.to_string());
                inbox.binding_nonce = Some(binding_nonce.to_string());
                inbox.last_error = None;
                inbox.updated_at = now_ms;
                claimed.push(request.clone());
            }
            Ok(claimed)
        })
    }

    /// A transport failure happens after a lease was claimed. Return it to the
    /// durable queue so a replacement can replay it; never strand it in progress.
    pub fn release_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        session_id: &str,
        binding_nonce: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter_mut()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            if request.state != PlannerRequestState::InProgress
                || request.delivered_to_session_id.as_deref() != Some(session_id)
                || request.delivered_with_binding_nonce.as_deref() != Some(binding_nonce)
            {
                return Ok(());
            }
            request.state = PlannerRequestState::Queued;
            request.accepted_at = None;
            request.lease_expires_at = None;
            request.delivered_to_tab_id = None;
            request.delivered_to_session_id = None;
            request.delivered_with_binding_nonce = None;
            if let Some(inbox) = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
            {
                inbox.phase = PlannerDeliveryPhase::PreSend;
                inbox.tab_id = None;
                inbox.session_id = None;
                inbox.binding_nonce = None;
                inbox.transport = None;
                inbox.last_error = None;
                inbox.updated_at = now_ms;
            }
            Ok(())
        })
    }

    pub fn start_planner_request_delivery(
        &self,
        plan_id: &str,
        request_id: &str,
        session_id: &str,
        binding_nonce: &str,
        transport: PlannerDeliveryTransport,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            if request.state != PlannerRequestState::InProgress
                || request.delivered_to_session_id.as_deref() != Some(session_id)
                || request.delivered_with_binding_nonce.as_deref() != Some(binding_nonce)
            {
                return Ok(());
            }
            let inbox = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            if inbox.phase == PlannerDeliveryPhase::PreSend {
                inbox.phase = PlannerDeliveryPhase::SendStarted;
                inbox.transport = Some(transport);
                inbox.updated_at = now_ms;
            }
            Ok(())
        })
    }

    pub fn mark_planner_delivery_uncertain(
        &self,
        plan_id: &str,
        request_id: &str,
        error: &str,
        observed_session_id: Option<&str>,
        observed_binding_nonce: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter_mut()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            request.lease_expires_at = None;
            let inbox = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            inbox.phase = PlannerDeliveryPhase::DeliveryUncertain;
            if let Some(session_id) = observed_session_id {
                inbox.session_id = Some(session_id.to_string());
            }
            if let Some(binding_nonce) = observed_binding_nonce {
                inbox.binding_nonce = Some(binding_nonce.to_string());
            }
            inbox.last_error = Some(error.to_string());
            inbox.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn accept_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        session_id: &str,
        binding_nonce: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter_mut()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            if request.state != PlannerRequestState::InProgress
                || request.delivered_to_session_id.as_deref() != Some(session_id)
                || request.delivered_with_binding_nonce.as_deref() != Some(binding_nonce)
            {
                return Ok(());
            }
            request.state = PlannerRequestState::Accepted;
            request.accepted_at = Some(now_ms);
            request.lease_expires_at = Some(now_ms.saturating_add(DELIVERY_LEASE_MS));
            let inbox = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            inbox.phase = PlannerDeliveryPhase::ProviderAccepted;
            inbox.last_error = None;
            inbox.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn complete_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        now_ms: u64,
    ) -> Result<PlannerRequest, PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter_mut()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            request.state = PlannerRequestState::Completed;
            request.completed_at = Some(now_ms);
            request.lease_expires_at = None;
            let completed = request.clone();
            let inbox = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            inbox.phase = PlannerDeliveryPhase::PlannerAcked;
            inbox.last_error = None;
            inbox.updated_at = now_ms;
            Ok(completed)
        })
    }

    pub fn fail_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_runtime_rows::<PlannerRows, _>(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter_mut()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            request.state = PlannerRequestState::Failed;
            request.failed_at = Some(now_ms);
            request.lease_expires_at = None;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
