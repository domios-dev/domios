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
        pane_id: &str,
        session_id: &str,
        binding_nonce: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(PlannerRef::Pane {
                pane_id: bound,
                title,
                ..
            }) = &plan.planner
            else {
                return Ok(false);
            };
            if bound != pane_id {
                return Ok(false);
            }
            plan.planner = Some(PlannerRef::Pane {
                pane_id: pane_id.to_string(),
                session_id: Some(session_id.to_string()),
                title: title.clone(),
            });
            for request in &mut plan.planner_requests {
                if request.state != PlannerRequestState::InProgress
                    || request.delivered_to_pane_id.as_deref() != Some(pane_id)
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
        caller_pane_id: Option<&str>,
        caller_session_id: Option<&str>,
        now_ms: u64,
    ) -> Result<PlannerRequest, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
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
                caller_pane_id: caller_pane_id.map(str::to_string),
                caller_session_id: caller_session_id.map(str::to_string),
                state: PlannerRequestState::Queued,
                created_at: now_ms,
                accepted_at: None,
                lease_expires_at: None,
                delivery_attempts: 0,
                completed_at: None,
                failed_at: None,
                delivered_to_pane_id: None,
                delivered_to_session_id: None,
                delivered_with_binding_nonce: None,
            };
            plan.planner_requests.push(request.clone());
            plan.planner_inbox.push(PlannerInboxEntry {
                request_id: request.request_id.clone(),
                phase: PlannerDeliveryPhase::PreSend,
                pane_id: None,
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
        pane_id: &str,
        session_id: &str,
        binding_nonce: &str,
        now_ms: u64,
    ) -> Result<Vec<PlannerRequest>, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            for request in &plan.planner_requests {
                if !plan
                    .planner_inbox
                    .iter()
                    .any(|entry| entry.request_id == request.request_id)
                {
                    plan.planner_inbox.push(PlannerInboxEntry {
                        request_id: request.request_id.clone(),
                        phase: PlannerDeliveryPhase::PreSend,
                        pane_id: None,
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
                request.delivered_to_pane_id = Some(pane_id.to_string());
                request.delivered_to_session_id = Some(session_id.to_string());
                request.delivered_with_binding_nonce = Some(binding_nonce.to_string());
                inbox.pane_id = Some(pane_id.to_string());
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
        self.with_plan(plan_id, now_ms, |plan| {
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
            request.delivered_to_pane_id = None;
            request.delivered_to_session_id = None;
            request.delivered_with_binding_nonce = None;
            if let Some(inbox) = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
            {
                inbox.phase = PlannerDeliveryPhase::PreSend;
                inbox.pane_id = None;
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
        self.with_plan(plan_id, now_ms, |plan| {
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
        self.with_plan(plan_id, now_ms, |plan| {
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
        self.with_plan(plan_id, now_ms, |plan| {
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
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let request = plan
                .planner_requests
                .iter_mut()
                .find(|request| request.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            request.state = PlannerRequestState::Completed;
            request.completed_at = Some(now_ms);
            request.lease_expires_at = None;
            let inbox = plan
                .planner_inbox
                .iter_mut()
                .find(|entry| entry.request_id == request_id)
                .ok_or(PlanError::NotFound)?;
            inbox.phase = PlannerDeliveryPhase::PlannerAcked;
            inbox.last_error = None;
            inbox.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn fail_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
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
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan, PlannerRef};

    #[test]
    fn post_open_binding_retargets_pre_send_lease_and_accepts_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.planner = Some(PlannerRef::Pane {
            pane_id: "planner".into(),
            session_id: Some("preflight-session".into()),
            title: Some("Planner".into()),
        });
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
            .unwrap();
        store
            .claim_planner_requests(
                &plan.id,
                "planner",
                "preflight-session",
                "preflight-nonce",
                3,
            )
            .unwrap();

        assert!(store
            .rebind_planner_session(&plan.id, "planner", "opened-session", "opened-nonce", 4,)
            .unwrap());
        let rebound = store.get(&plan.id).unwrap();
        assert!(matches!(
            rebound.planner,
            Some(PlannerRef::Pane { session_id: Some(ref id), .. }) if id == "opened-session"
        ));
        let request = &rebound.planner_requests[0];
        assert_eq!(
            request.delivered_to_session_id.as_deref(),
            Some("opened-session")
        );
        assert_eq!(
            request.delivered_with_binding_nonce.as_deref(),
            Some("opened-nonce")
        );

        store
            .start_planner_request_delivery(
                &plan.id,
                "req",
                "opened-session",
                "opened-nonce",
                PlannerDeliveryTransport::ChatBackend,
                5,
            )
            .unwrap();
        store
            .accept_planner_request(&plan.id, "req", "preflight-session", "preflight-nonce", 6)
            .unwrap();
        assert_eq!(
            store.get(&plan.id).unwrap().planner_requests[0].state,
            PlannerRequestState::InProgress
        );
        store
            .accept_planner_request(&plan.id, "req", "opened-session", "opened-nonce", 7)
            .unwrap();
        assert!(store
            .claim_planner_requests(
                &plan.id,
                "replacement",
                "replacement-session",
                "replacement-nonce",
                DELIVERY_LEASE_MS + 8,
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rotation_after_send_start_records_observed_identity_without_replay() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
            .unwrap();
        store
            .claim_planner_requests(&plan.id, "planner", "sent-session", "sent-nonce", 3)
            .unwrap();
        store
            .start_planner_request_delivery(
                &plan.id,
                "req",
                "sent-session",
                "sent-nonce",
                PlannerDeliveryTransport::ChatBackend,
                4,
            )
            .unwrap();
        store
            .mark_planner_delivery_uncertain(
                &plan.id,
                "req",
                "provider binding rotated after send started",
                Some("observed-session"),
                Some("observed-nonce"),
                5,
            )
            .unwrap();

        let persisted = store.get(&plan.id).unwrap();
        let request = &persisted.planner_requests[0];
        let inbox = &persisted.planner_inbox[0];
        assert_eq!(
            request.delivered_to_session_id.as_deref(),
            Some("sent-session")
        );
        assert_eq!(inbox.session_id.as_deref(), Some("observed-session"));
        assert_eq!(inbox.binding_nonce.as_deref(), Some("observed-nonce"));
        assert_eq!(inbox.phase, PlannerDeliveryPhase::DeliveryUncertain);
        assert!(store
            .claim_planner_requests(
                &plan.id,
                "planner",
                "observed-session",
                "observed-nonce",
                DELIVERY_LEASE_MS + 6,
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn requests_are_durable_and_claimed_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_request".into();
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(
                &plan.id,
                "req_1".into(),
                "one\n<none>".into(),
                Some("caller"),
                Some("sid"),
                2,
            )
            .unwrap();
        assert_eq!(
            store
                .claim_planner_requests(&plan.id, "planner", "sid-p", "nonce-p", 3)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .claim_planner_requests(&plan.id, "planner", "sid-p", "nonce-p", 4)
            .unwrap()
            .is_empty());
        store
            .accept_planner_request(&plan.id, "req_1", "sid-p", "nonce-p", 4)
            .unwrap();
        store
            .complete_planner_request(&plan.id, "req_1", 5)
            .unwrap();
        assert_eq!(
            store.get(&plan.id).unwrap().planner_requests[0].state,
            PlannerRequestState::Completed
        );
    }

    #[test]
    fn replacement_claims_an_abandoned_lease_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_replacement".into();
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
            .unwrap();
        assert_eq!(
            store
                .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 3)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 4)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .claim_planner_requests(&plan.id, "new", "sid-new", "nonce-new", 5)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .claim_planner_requests(&plan.id, "new", "sid-new", "nonce-new", 6)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn provider_accepted_delivery_waits_for_ack_without_automatic_replay() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_ack".into();
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
            .unwrap();
        store
            .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 3)
            .unwrap();
        store
            .accept_planner_request(&plan.id, "req_1", "sid-old", "nonce-old", 4)
            .unwrap();

        assert!(store
            .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 5)
            .unwrap()
            .is_empty());
        assert!(store
            .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-new", 6)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn expired_in_progress_lease_replays_once_to_same_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_lease".into();
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
            .unwrap();
        store
            .claim_planner_requests(&plan.id, "planner", "sid", "nonce", 3)
            .unwrap();

        assert!(store
            .claim_planner_requests(&plan.id, "planner", "sid", "nonce", DELIVERY_LEASE_MS + 2,)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .claim_planner_requests(&plan.id, "planner", "sid", "nonce", DELIVERY_LEASE_MS + 3,)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn provider_accepted_delivery_never_replays_after_lease_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_accepted_lease".into();
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
            .unwrap();
        store
            .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 3)
            .unwrap();
        store
            .accept_planner_request(&plan.id, "req_1", "sid-old", "nonce-old", 4)
            .unwrap();

        let replacement = store
            .claim_planner_requests(
                &plan.id,
                "new",
                "sid-new",
                "nonce-new",
                DELIVERY_LEASE_MS + 5,
            )
            .unwrap();
        assert!(replacement.is_empty());
        store
            .release_planner_request(&plan.id, "req_1", "sid-old", "nonce-old", 6)
            .unwrap();
        let request = &store.get(&plan.id).unwrap().planner_requests[0];
        assert_eq!(request.state, PlannerRequestState::Accepted);
        assert_eq!(request.delivered_to_session_id.as_deref(), Some("sid-old"));
    }

    #[test]
    fn powershell_and_posix_payload_is_stored_as_data_without_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_shell_data".into();
        store.save(&mut plan, 1).unwrap();
        let payload =
            "- audit `model`\n- <nonexistent>\n$env:BAD='x'; echo $(whoami)\n$(false) && :";
        let request = store
            .queue_planner_request(
                &plan.id,
                "req_shell".into(),
                payload.into(),
                Some("caller"),
                Some("caller-sid"),
                2,
            )
            .unwrap();
        assert_eq!(request.payload, payload);
        assert_eq!(
            store.get(&plan.id).unwrap().planner_requests[0].payload,
            payload
        );
    }

    #[test]
    fn pty_and_chatbackend_crash_windows_never_resend_uncertain_delivery() {
        for transport in [
            PlannerDeliveryTransport::PtyAgent,
            PlannerDeliveryTransport::ChatBackend,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let store = PlanStore::new_for_test(dir.path());
            let mut plan = Plan::new(
                "P".into(),
                dir.path().display().to_string(),
                OrchestratorRef::default(),
            );
            plan.id = format!("pl_{transport:?}");
            store.save(&mut plan, 1).unwrap();
            store
                .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
                .unwrap();

            store
                .claim_planner_requests(&plan.id, "planner", "sid", "nonce", 3)
                .unwrap();
            store
                .start_planner_request_delivery(&plan.id, "req", "sid", "nonce", transport, 4)
                .unwrap();
            assert!(store
                .claim_planner_requests(
                    &plan.id,
                    "replacement",
                    "sid-new",
                    "nonce-new",
                    DELIVERY_LEASE_MS + 5,
                )
                .unwrap()
                .is_empty());
            let plan = store.get(&plan.id).unwrap();
            let inbox = &plan.planner_inbox[0];
            assert_eq!(inbox.phase, PlannerDeliveryPhase::DeliveryUncertain);
            assert_eq!(inbox.transport, Some(transport));
            assert!(inbox
                .last_error
                .as_deref()
                .unwrap()
                .contains("send started"));
        }
    }

    #[test]
    fn provider_accept_and_planner_ack_are_durable_inbox_phases() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
            .unwrap();
        store
            .claim_planner_requests(&plan.id, "planner", "sid", "nonce", 3)
            .unwrap();
        store
            .start_planner_request_delivery(
                &plan.id,
                "req",
                "sid",
                "nonce",
                PlannerDeliveryTransport::ChatBackend,
                4,
            )
            .unwrap();
        store
            .accept_planner_request(&plan.id, "req", "sid", "nonce", 5)
            .unwrap();
        assert_eq!(
            store.get(&plan.id).unwrap().planner_inbox[0].phase,
            PlannerDeliveryPhase::ProviderAccepted
        );
        store.complete_planner_request(&plan.id, "req", 6).unwrap();
        assert_eq!(
            store.get(&plan.id).unwrap().planner_inbox[0].phase,
            PlannerDeliveryPhase::PlannerAcked
        );
    }
}
