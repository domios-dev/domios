use super::*;

impl super::super::ServiceContext {
    pub(crate) fn dispatch_ready_user_send(&self, record: &UserSendRecord) {
        self.wake_user_owner(&record.authority.tab_id, UserWake::Admission);
    }

    pub fn drain_user_owner(&self, tab_id: &str) -> Result<UserDrain, String> {
        if super::super::agent_runtime::requested_for_tab(self, tab_id) {
            return super::super::agent_runtime::user_delivery::drain(self, tab_id);
        }
        let owner = self.pending_delivery_payloads.user_sends.tab(tab_id);
        let _drain = owner.pty_drain.lock();
        if self
            .pending_delivery_payloads
            .user_sends
            .tab(tab_id)
            .revoked
            .load(Ordering::Acquire)
        {
            if let Some(head) = self.pending_delivery_payloads.user_sends.head(tab_id)? {
                return Ok(self.pending_user_send(&head, "owner_revoked"));
            }
            return Ok(UserDrain::Idle);
        }
        if let Err(outcome) = self.deliver_parked_bootstrap_record(tab_id) {
            return Ok(outcome);
        }
        self.drain_user_owner_with(tab_id, |record, text, readiness, capture_send| {
            self.submit_owned_user(record, text, readiness, capture_send)
        })
    }

    pub(super) fn drain_user_owner_with(
        &self,
        tab_id: &str,
        mut submit: impl FnMut(
            &UserSendRecord,
            &str,
            Option<&UserReadiness>,
            bool,
        ) -> Result<UserWrite, DeliveryError>,
    ) -> Result<UserDrain, String> {
        let store = &self.pending_delivery_payloads.user_sends;
        for _ in 0..crate::service::pending_delivery_payloads::user_sends::MAX_UNSETTLED * 2 {
            let Some(mut head) = store.head(tab_id)? else {
                return Ok(UserDrain::Idle);
            };
            if head.state != UserSendState::Queued {
                return Ok(UserDrain::Idle);
            }
            self.chat_send_ledger.restore_user_record(&head);
            if let Some(carried) = self.carry_held_user_send(&head)? {
                head = carried;
            }
            if self.settle_degraded_user_send(&head)? {
                continue;
            }
            if !self.user_authority_is_ready(&head.authority)
                && self.held_user_identity_is_unresolved(&head)
            {
                return Ok(self.pending_user_send(&head, "held_launch_identity_pending"));
            }
            if let Some(rebound) = self.advance_prelaunch_user_authority(&head)? {
                head = rebound;
            }
            let same_launch = self
                .mux
                .read()
                .get_tab(tab_id)
                .is_some_and(|tab| self.user_authority_matches_launch(tab, &head.authority));
            let ended_launch = self.user_authority_is_from_ended_launch(&head.authority);
            if head.state == UserSendState::Queued && (!same_launch || ended_launch) {
                if ended_launch {
                    match store.discard_queued_from_ended_launch(&head) {
                        Ok(true) => {
                            const REASON: &str = "launch_ended_before_write";
                            if let Some(status) = self
                                .chat_send_ledger
                                .expire_owned_before_write(&head.send_id, REASON)
                            {
                                crate::events::emit_everywhere(
                                    &self.app_handle,
                                    "chat-send-status",
                                    &status,
                                );
                            } else {
                                log::warn!(
                                    "[chat-send] expired user send {} had no held ledger status",
                                    head.send_id
                                );
                            }
                            log::warn!(
                                "[chat-send] user owner settled tab={} send_id={} reason={REASON}",
                                head.authority.tab_id,
                                head.send_id,
                            );
                            self.local_recordings.retire_write_ticket(&head.send_id);
                            continue;
                        }
                        Ok(false) => {
                            return Ok(
                                self.pending_user_send(&head, "ended_launch_settlement_changed")
                            );
                        }
                        Err(error) => {
                            log::warn!(
                                "[chat-send] user send {} launch settlement failed: {error}",
                                head.send_id
                            );
                            return Ok(
                                self.pending_user_send(&head, "ended_launch_settlement_failed")
                            );
                        }
                    }
                }
                if self.local_recordings.has_write_ticket(&head.send_id)
                    && store.discard_queued_from_ended_launch(&head)?
                {
                    if let Some(status) = self.chat_send_ledger.expire_owned_before_write(
                        &head.send_id,
                        "recording_recipient_changed_before_write",
                    ) {
                        crate::events::emit_everywhere(
                            &self.app_handle,
                            "chat-send-status",
                            &status,
                        );
                    }
                    self.local_recordings.retire_write_ticket(&head.send_id);
                    continue;
                }
                return Ok(self.pending_user_send(&head, "user_launch_identity_unavailable"));
            }
            if head.authority.pty_session_generation.is_none()
                && head.state == UserSendState::Queued
            {
                let Some(pty_id) = head.authority.pty_id.as_deref() else {
                    return Ok(self.pending_user_send(&head, "pty_authority_pending"));
                };
                let generation = self
                    .pty
                    .lock()
                    .get_session(pty_id)
                    .map(|session| session.generation);
                let Some(generation) = generation else {
                    return Ok(self.pending_user_send(&head, "pty_session_generation_unavailable"));
                };
                let Some(bound) = store.bind_pty_session_generation(&head, generation)? else {
                    return Ok(self.pending_user_send(
                        &head,
                        "pty_session_generation_changed_before_binding",
                    ));
                };
                if !self
                    .chat_send_ledger
                    .bind_user_pty_session_generation(&head.send_id, &bound.authority)
                {
                    return Ok(
                        self.pending_user_send(&head, "user_pty_generation_binding_mismatch")
                    );
                }
                head = bound;
            }
            let readiness = if self.user_authority_is_ready(&head.authority) {
                None
            } else {
                match self.user_readiness_fallback(&head.authority) {
                    Ok(readiness) => Some(readiness),
                    Err(reason) => return Ok(self.pending_user_send(&head, reason)),
                }
            };
            if !store.eligible(&head) {
                return Ok(self.pending_user_send(&head, "owner_head_not_eligible"));
            }
            let text = store.prepared(&head)?;
            let separator_before = if head.held_before_readiness {
                self.chat_send_ledger
                    .has_held_user_send_before(&head.authority, head.ledger_sequence)
            } else {
                self.chat_send_ledger
                    .has_unsettled_user_send_before(&head.authority, head.ledger_sequence)
            };
            let text = if separator_before {
                format!("\n\n{text}")
            } else {
                text
            };
            let evidence = self.chat_send_anchor(tab_id);
            if evidence
                .anchor
                .as_ref()
                .is_some_and(|anchor| anchor.provider_id != head.authority.provider_id)
                || evidence
                    .live_binding
                    .as_ref()
                    .and_then(|binding| binding.binding_nonce.as_deref())
                    .is_some_and(|nonce| nonce != head.authority.binding_nonce)
                || (!self.user_authority_is_ready(&head.authority)
                    && readiness.as_ref().is_none_or(|receipt| {
                        !self.user_readiness_is_current(&head.authority, receipt)
                    }))
            {
                return Ok(self.pending_user_send(&head, "launch_binding_changed_before_claim"));
            }
            let turn_text = format!(
                "{text}{}",
                head.typed_follow_up.as_deref().unwrap_or_default()
            );
            let session = evidence
                .live_binding
                .as_ref()
                .and_then(|binding| binding.session_id.as_deref());
            let submit_claim = if let Some(pty_id) = head.authority.pty_id.as_deref() {
                let claim = self
                    .chat_send_ledger
                    .take_user_submit_claim(&head.send_id)
                    .unwrap_or_else(|| {
                        crate::service::pty_io::submit_transaction::claim_submit_cycle(pty_id)
                    });
                if let Some(wait) = claim.command_wait() {
                    let dispatch_generation = store
                        .tab(tab_id)
                        .dispatch_generation
                        .load(Ordering::Acquire);
                    self.hold_user_send_for_model_command(tab_id, &head.send_id, wait);
                    return Ok(UserDrain::ZeroWriteRefusal {
                        dispatch_generation,
                    });
                }
                Some(claim)
            } else {
                None
            };
            if !store.claim(&head, session)? {
                return Ok(self.pending_user_send(&head, "owner_claim_refused"));
            }
            if !self
                .chat_send_ledger
                .begin_user_handoff(&head, &turn_text, evidence)
            {
                return Err(
                    "persisted handoff has no joined ledger admission; held uncertainty".into(),
                );
            }
            self.clear_user_pending_reason(&head);
            let claimed = store
                .records(tab_id)?
                .into_iter()
                .find(|record| record.send_id == head.send_id)
                .ok_or("durable user claim disappeared")?;
            let dispatch_generation = store
                .tab(tab_id)
                .dispatch_generation
                .load(Ordering::Acquire);
            let capture_send = self.local_recordings.has_write_ticket(&head.send_id);
            let outcome =
                if self.user_send_readiness_is_current(&claimed.authority, readiness.as_ref()) {
                    if let Some(claim) = submit_claim {
                        self.chat_send_ledger
                            .store_user_submit_claim(&head.send_id, claim);
                    }
                    submit(&claimed, &text, readiness.as_ref(), capture_send)
                } else {
                    self.pending_user_send(&claimed, "readiness_changed_before_payload_write");
                    Err(DeliveryError::TabBusyRefused {
                        tab: tab_id.to_string(),
                        detail: "user owner authority revoked before handoff".into(),
                    })
                };
            self.chat_send_ledger.take_user_submit_claim(&head.send_id);
            let zero_write = outcome
                .as_ref()
                .err()
                .is_some_and(|error| !error.bytes_written())
                && !self
                    .chat_send_ledger
                    .user_payload_may_have_been_written(&head.send_id);
            store.finish_handoff(&head, zero_write)?;
            if zero_write {
                self.chat_send_ledger.zero_write_user_handoff(&head.send_id);
            }
            if !matches!(outcome, Ok(UserWrite::Enqueued)) {
                self.finish_user_write(&head.send_id, outcome.map(|_| ()));
            }
            if zero_write {
                let reason = self
                    .chat_send_ledger
                    .status(&head.send_id)
                    .and_then(|status| status.pending_reason)
                    .unwrap_or_else(|| "payload_not_written".into());
                if capture_send {
                    store.discard_queued_from_ended_launch(&head)?;
                    self.local_recordings.retire_write_ticket(&head.send_id);
                    continue;
                }
                self.pending_user_send(&head, &reason);
                return Ok(UserDrain::ZeroWriteRefusal {
                    dispatch_generation,
                });
            }
            if capture_send {
                self.local_recordings.retire_write_ticket(&head.send_id);
            }
            if !store.complete(&head)? {
                log::warn!(
                    "[chat-send] written user send {} payload remains retained as uncertain",
                    head.send_id
                );
            }
        }
        Ok(UserDrain::Idle)
    }
}
