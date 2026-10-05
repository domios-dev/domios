use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};

fn take_parked_user_send(
    pending: &mut Option<PendingDeliveryRecord>,
    record_id: &str,
    send_id: Option<&str>,
) -> Option<PendingDeliveryRecord> {
    let record = pending.as_ref()?;
    if record.channel != PendingDeliveryChannel::InteractiveInput
        || record.record_id() != record_id
        || record.send_id() != send_id
        || record.state() != PendingDeliveryState::Pending
    {
        return None;
    }
    pending.take()
}

fn return_after_send_failure(
    pending: &mut Option<PendingDeliveryRecord>,
    record: PendingDeliveryRecord,
    bytes_written: bool,
) -> Result<(), PendingDeliveryRecord> {
    if bytes_written
        || pending
            .as_ref()
            .is_some_and(|current| current.record_id() != record.record_id())
    {
        return Err(record);
    }
    *pending = Some(record);
    Ok(())
}

use std::sync::atomic::Ordering;
use super::pending_delivery_payloads::user_sends::{UserSendAuthority, UserSendRecord, UserSendState};
use super::delivery_error::DeliveryError;

pub(crate) enum UserDrain {
    Idle,
    ZeroWriteRefusal { dispatch_generation: u64 },
}

type UserReadiness = super::supporter_tab_state::UserDeliveryReadiness;

fn requires_observed_session_id_for_readiness(
    observed_session_id: Option<&str>,
    session_starts_on_first_user_turn: bool,
) -> bool {
    observed_session_id.is_none() && !session_starts_on_first_user_turn
}

fn supporter_pending_reason(error: &str) -> &'static str {
    match error {
        "supporter_adapter_unavailable" => "supporter_adapter_unavailable",
        "supporter_observation_not_queued" => "supporter_observation_not_queued",
        "supporter_observation_deadline" => "supporter_observation_deadline",
        "supporter_observation_cancelled" => "supporter_observation_cancelled",
        "supporter_observation_unavailable" => "supporter_observation_unavailable",
        "supporter_pty_generation_changed_during_capture" => "supporter_pty_generation_changed",
        "user delivery PTY session generation unavailable" => "pty_session_generation_unavailable",
        _ => "supporter_observation_capture_unavailable",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserWake {
    Readiness,
    Admission,
    Housekeeping,
}

impl super::ServiceContext {
    pub(crate) fn user_send_authority(&self, tab_id: &str) -> Result<UserSendAuthority, String> {
        let (pty_id, binding_nonce, provider_id, session_id, observed_session_id) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("user send tab is absent")?;
            if !matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty) {
                return Err("user send owner requires PTY backing".into());
            }
            let session_id = tab.agent_session_id.clone();
            (
                tab.pty_id.clone().ok_or("user send has no exact PTY authority")?,
                tab.session_binding_nonce.clone().ok_or("user send has no launch nonce")?,
                tab.agent_type.clone().ok_or("user send has no agent identity")?,
                tab.launch_session_authenticated().then(|| session_id.clone()).flatten(),
                session_id,
            )
        };
        let pty_session_generation = self.pty.lock().get_session(&pty_id).map(|session| session.generation);
        Ok(UserSendAuthority {
            tab_id: tab_id.to_string(),
            pty_id,
            binding_nonce,
            pty_session_generation,
            provider_id,
            session_id,
            observed_session_id,
        })
    }

    pub(crate) fn user_authority_is_ready(&self, authority: &UserSendAuthority) -> bool {
        let owner = self.pending_delivery_payloads.user_sends.tab(&authority.tab_id);
        if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) { return false; }
        if !self.user_authority_pty_generation_is_current(authority) { return false; }
        if !self.mux.read().get_tab(&authority.tab_id)
            .is_some_and(|tab| self.user_authority_matches_launch(tab, authority)) {
            return false;
        }
        self.pending_delivery_payloads.user_sends
            .ready_launch_is_latched(authority)
    }

    pub(crate) fn latch_user_authority_ready(&self, authority: &UserSendAuthority) -> bool {
        if !self.user_authority_pty_generation_is_current(authority) {
            return false;
        }
        let is_current_agent_ready = self.mux.read().get_tab(&authority.tab_id).is_some_and(|tab| {
            self.user_authority_matches_launch(tab, authority)
                && tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
        });
        is_current_agent_ready
            && self.rearm_user_input_for_authority(authority)
            && self.pending_delivery_payloads.user_sends.latch_ready_launch(authority)
    }

    fn rearm_user_input_for_authority(&self, authority: &UserSendAuthority) -> bool {
        let Some(generation) = authority.pty_session_generation else { return false; };
        let pty = self.pty.lock();
        pty.session_generation_matches(&authority.pty_id, generation)
            && pty.rearm_guarded_input(&authority.pty_id).is_ok()
    }

    pub(crate) fn user_authority_is_current_session(
        &self,
        authority: &UserSendAuthority,
        session_id: &str,
        binding_nonce: &str,
    ) -> bool {
        self.user_authority_pty_generation_is_current(authority)
            && self.mux.read().get_tab(&authority.tab_id).is_some_and(|tab| {
                self.user_authority_matches_launch(tab, authority)
                    && tab.launch_session_authenticated()
                    && tab.agent_session_id.as_deref() == Some(session_id)
                    && tab.reported_session_binding_nonce.as_deref() == Some(binding_nonce)
                    && authority.binding_nonce == binding_nonce
            })
    }

    fn user_authority_matches_launch(
        &self,
        tab: &crate::multiplexer::types::Tab,
        authority: &UserSendAuthority,
    ) -> bool {
        matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty)
            && tab.pty_id.as_deref() == Some(authority.pty_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(authority.binding_nonce.as_str())
            && tab.agent_type.as_deref() == Some(authority.provider_id.as_str())
    }

    fn user_authority_is_from_ended_launch(&self, authority: &UserSendAuthority) -> bool {
        self.user_send_authority(&authority.tab_id).is_ok_and(|current| {
            current.pty_id != authority.pty_id
                || current.binding_nonce != authority.binding_nonce
                || current.provider_id != authority.provider_id
        })
    }

    fn user_authority_pty_generation_is_current(&self, authority: &UserSendAuthority) -> bool {
        authority.pty_session_generation.is_some_and(|generation| {
            self.pty.lock().session_generation_matches(&authority.pty_id, generation)
        })
    }

    fn pending_user_send(&self, record: &UserSendRecord, reason: &str) -> UserDrain {
        log::warn!(
            "[chat-send] user owner pending tab={} send_id={} reason={reason}",
            record.authority.tab_id,
            record.send_id,
        );
        if let Some(status) = self.chat_send_ledger.set_user_pending_reason(&record.send_id, Some(reason)) {
            crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
        UserDrain::Idle
    }

    fn clear_user_pending_reason(&self, record: &UserSendRecord) {
        if let Some(status) = self.chat_send_ledger.set_user_pending_reason(&record.send_id, None) {
            crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
    }

    fn user_readiness_fallback(
        &self,
        authority: &UserSendAuthority,
    ) -> Result<UserReadiness, &'static str> {
        let (same_launch, binding_authenticated) = {
            let mux = self.mux.read();
            mux.get_tab(&authority.tab_id).map_or((false, false), |tab| {
                (self.user_authority_matches_launch(tab, authority), tab.launch_session_authenticated())
            })
        };
        if !same_launch { return Err("launch_generation_changed"); }
        if !self.user_authority_pty_generation_is_current(authority) {
            return Err("pty_session_generation_unavailable_or_changed");
        }
        if binding_authenticated { return Err("exact_binding_present_stage_pending"); }
        let session_starts_on_first_user_turn = self
            .agent_registry
            .with_provider(&authority.provider_id, |provider| {
                provider.session_starts_on_first_user_turn()
            })
            .unwrap_or(false);
        if requires_observed_session_id_for_readiness(
            authority.observed_session_id.as_deref(),
            session_starts_on_first_user_turn,
        ) {
            return Err("resume_session_unknown");
        }
        let first = self
            .observe_user_delivery_readiness(&authority.tab_id, None)
            .map_err(|error| supporter_pending_reason(&error))?;
        if let Some(reason) = first.pending_reason() { return Err(reason); }
        if !self.user_delivery_observation_is_current(&first) {
            return Err("supporter_first_observation_stale");
        }
        let second = self
            .observe_user_delivery_readiness(&authority.tab_id, Some(&first))
            .map_err(|error| supporter_pending_reason(&error))?;
        let pair = UserReadiness::new(first, second).map_err(|reason| reason)?;
        let current_launch = {
            let mux = self.mux.read();
            mux.get_tab(&authority.tab_id)
                .is_some_and(|tab| self.user_authority_matches_launch(tab, authority))
        };
        if !current_launch { return Err("launch_generation_changed"); }
        if !self.user_delivery_observation_is_current(pair.current_observation()) {
            return Err("supporter_second_observation_stale");
        }
        Ok(pair)
    }

    pub(crate) fn park_refused_user_send(&self, tab_id: &str, send_id: &str, text: &str, error: &DeliveryError) {
        if error.bytes_written() || text.is_empty() { return; }
        let result = (|| {
            let accepted = self.chat_send_ledger.status(send_id).ok_or("user send ledger admission absent")?;
            let reservation = self.pending_delivery_payloads.user_sends.reserve(
                self.user_send_authority(tab_id)?, accepted.correlation_id,
                || (send_id.to_string(), accepted.sequence))?;
            let record = self.pending_delivery_payloads.user_sends.admit(&reservation, text, text, None)?;
            self.chat_send_ledger.admit_user(&record);
            if !self.user_authority_is_ready(&record.authority) {
                self.chat_send_ledger.set_user_pending_reason(&record.send_id, Some("awaiting_agent_ready"));
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result { log::warn!("[chat-send] tab={tab_id} admission held: {error}"); }
    }

    pub(crate) fn wake_parked_user_send(&self, tab_id: &str) {
        self.wake_user_owner(tab_id, UserWake::Readiness);
    }

    pub(crate) fn wake_user_owner(&self, tab_id: &str, reason: UserWake) {
        let owner = self.pending_delivery_payloads.user_sends.tab(tab_id);
        if reason != UserWake::Housekeeping { owner.dispatch_generation.fetch_add(1, Ordering::AcqRel); }
        owner.wake_generation.fetch_add(1, Ordering::AcqRel);
        if owner.revoked.load(Ordering::Acquire)
            || owner.running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return;
        }
        let service = self.clone();
        let tab_id = tab_id.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            loop {
                let generation = owner.wake_generation.load(Ordering::Acquire);
                let refusal_generation = match service.drain_user_owner(&tab_id) {
                    Ok(UserDrain::ZeroWriteRefusal { dispatch_generation }) => Some(dispatch_generation),
                    Ok(UserDrain::Idle) => None,
                    Err(error) => {
                        log::warn!("[chat-send] tab={tab_id} owner held: {error}");
                        None
                    },
                };
                let changed = || match refusal_generation {
                    Some(refused_at) => owner.dispatch_generation.load(Ordering::Acquire) != refused_at,
                    None => owner.wake_generation.load(Ordering::Acquire) != generation,
                };
                if !owner.revoked.load(Ordering::Acquire) && changed() { continue; }
                owner.running.store(false, Ordering::Release);
                if owner.revoked.load(Ordering::Acquire) || !changed()
                    || owner.running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                    break;
                }
            }
            #[cfg(test)]
            tests::notify_owner_idle(&tab_id);
        });
    }

    pub(crate) fn deliver_parked_user_send(&self, tab_id: &str) {
        self.wake_parked_user_send(tab_id);
    }

    pub(crate) fn drain_user_owner(&self, tab_id: &str) -> Result<UserDrain, String> {
        if self.pending_delivery_payloads.user_sends.tab(tab_id).revoked.load(Ordering::Acquire) {
            if let Some(head) = self.pending_delivery_payloads.user_sends.head(tab_id)? {
                return Ok(self.pending_user_send(&head, "owner_revoked"));
            }
            return Ok(UserDrain::Idle);
        }
        if let Err(outcome) = self.deliver_parked_bootstrap_record(tab_id) { return Ok(outcome); }
        self.drain_user_owner_with(tab_id, |record, text, readiness| self.submit_owned_user(record, text, readiness))
    }

    fn drain_user_owner_with(
        &self,
        tab_id: &str,
        mut submit: impl FnMut(&UserSendRecord, &str, Option<&UserReadiness>) -> Result<(), DeliveryError>,
    ) -> Result<UserDrain, String> {
        let store = &self.pending_delivery_payloads.user_sends;
        for _ in 0..super::pending_delivery_payloads::user_sends::MAX_UNSETTLED * 2 {
            let Some(mut head) = store.queued(tab_id)?.into_iter().next() else { return Ok(UserDrain::Idle); };
            let same_launch = self
                .mux
                .read()
                .get_tab(tab_id)
                .is_some_and(|tab| self.user_authority_matches_launch(tab, &head.authority));
            if head.state == UserSendState::Queued && !same_launch {
                if self.user_authority_is_from_ended_launch(&head.authority) {
                    match store.discard_queued_from_ended_launch(&head) {
                        Ok(true) => {
                            const REASON: &str = "launch_ended_before_write";
                            if let Some(status) = self
                                .chat_send_ledger
                                .expire_owned_before_write(&head.send_id, REASON)
                            {
                                crate::api::events::emit_everywhere(
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
                            continue;
                        }
                        Ok(false) => {
                            return Ok(self.pending_user_send(
                                &head,
                                "ended_launch_settlement_changed",
                            ));
                        }
                        Err(error) => {
                            log::warn!(
                                "[chat-send] user send {} launch settlement failed: {error}",
                                head.send_id
                            );
                            return Ok(self.pending_user_send(
                                &head,
                                "ended_launch_settlement_failed",
                            ));
                        }
                    }
                }
                return Ok(self.pending_user_send(&head, "user_launch_identity_unavailable"));
            }
            if head.authority.pty_session_generation.is_none() && head.state == UserSendState::Queued {
                let generation = self.pty.lock().get_session(&head.authority.pty_id)
                    .map(|session| session.generation);
                let Some(generation) = generation else {
                    return Ok(self.pending_user_send(&head, "pty_session_generation_unavailable"));
                };
                let Some(bound) = store.bind_pty_session_generation(&head, generation)? else {
                    return Ok(self.pending_user_send(&head, "pty_session_generation_changed_before_binding"));
                };
                if !self.chat_send_ledger.bind_user_pty_session_generation(&head.send_id, &bound.authority) {
                    return Ok(self.pending_user_send(&head, "user_pty_generation_binding_mismatch"));
                }
                head = bound;
            }
            if head.held_before_readiness
                && self.chat_send_ledger.has_unsettled_user_send_before(
                    &head.authority,
                    head.ledger_sequence,
                    true,
                )
            {
                return Ok(UserDrain::Idle);
            }
            let readiness = if self.user_authority_is_ready(&head.authority) {
                None
            } else {
                match self.user_readiness_fallback(&head.authority) {
                    Ok(readiness) => Some(readiness),
                    Err(reason) => return Ok(self.pending_user_send(&head, reason)),
                }
            };
            if !store.eligible(&head) { return Ok(self.pending_user_send(&head, "owner_head_not_eligible")); }
            let text = store.prepared(&head)?;
            let separator_before = !head.held_before_readiness
                && self.chat_send_ledger.has_unsettled_user_send_before(
                    &head.authority,
                    head.ledger_sequence,
                    false,
                );
            let text = if separator_before {
                format!("\n\n{text}")
            } else {
                text
            };
            let evidence = self.chat_send_anchor(tab_id);
            if evidence.anchor.as_ref().is_some_and(|anchor| anchor.provider_id != head.authority.provider_id)
                || evidence.live_binding.as_ref().and_then(|binding| binding.binding_nonce.as_deref())
                    .is_some_and(|nonce| nonce != head.authority.binding_nonce)
                || (!self.user_authority_is_ready(&head.authority)
                    && readiness.as_ref().is_none_or(|receipt| !self.user_readiness_is_current(&head.authority, receipt))) {
                return Ok(self.pending_user_send(&head, "launch_binding_changed_before_claim"));
            }
            let turn_text = format!("{text}{}", head.typed_follow_up.as_deref().unwrap_or_default());
            let session = evidence.live_binding.as_ref().and_then(|binding| binding.session_id.as_deref());
            if !store.claim(&head, session)? { return Ok(self.pending_user_send(&head, "owner_claim_refused")); }
            if !self.chat_send_ledger.begin_user_handoff(&head, &turn_text, evidence) {
                return Err("persisted handoff has no joined ledger admission; held uncertainty".into());
            }
            self.clear_user_pending_reason(&head);
            let claimed = store.records(tab_id)?.into_iter()
                .find(|record| record.send_id == head.send_id)
                .ok_or("durable user claim disappeared")?;
            let dispatch_generation = store.tab(tab_id).dispatch_generation.load(Ordering::Acquire);
            let outcome = if self.user_send_readiness_is_current(&claimed.authority, readiness.as_ref()) {
                submit(&claimed, &text, readiness.as_ref())
            } else {
                self.pending_user_send(&claimed, "readiness_changed_before_payload_write");
                Err(DeliveryError::TabBusyRefused { tab: tab_id.to_string(), detail: "user owner authority revoked before handoff".into() })
            };
            let zero_write = outcome.as_ref().err().is_some_and(|error| !error.bytes_written())
                && !self.chat_send_ledger.user_payload_may_have_been_written(&head.send_id);
            store.finish_handoff(&head, zero_write)?;
            if zero_write { self.chat_send_ledger.zero_write_user_handoff(&head.send_id); }
            if let Some(status) = self.chat_send_ledger.finish(&head.send_id, outcome.map_err(|error| error.ledger_message())) {
                crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
            }
            if zero_write {
                let reason = self.chat_send_ledger.status(&head.send_id)
                    .and_then(|status| status.pending_reason)
                    .unwrap_or_else(|| "payload_not_written".into());
                self.pending_user_send(&head, &reason);
                return Ok(UserDrain::ZeroWriteRefusal { dispatch_generation });
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

    fn deliver_parked_bootstrap_record(&self, tab_id: &str) -> Result<(), UserDrain> {
        let dispatch_generation = self.pending_delivery_payloads.user_sends.tab(tab_id)
            .dispatch_generation.load(Ordering::Acquire);
        let ready = self.mux.read().get_tab(tab_id)
            .is_some_and(|tab| tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady));
        if !ready { return Ok(()); }
        let parked = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.as_ref())
            .filter(|record| {
                record.channel == PendingDeliveryChannel::InteractiveInput
                    && record.send_id().is_none()
                    && record.state() == PendingDeliveryState::Pending
            })
            .map(|record| {
                let record_id = record.record_id().to_string();
                let send_id = record.send_id().map(str::to_string);
                let delivery_id = send_id.as_deref().unwrap_or(&record_id).to_string();
                (
                    record_id,
                    send_id,
                    delivery_id,
                    record.content_digest.clone(),
                )
            });
        let Some((record_id, send_id, delivery_id, digest)) = parked else {
            return Ok(());
        };
        let text = match self.pending_delivery_payloads.resolve(&digest) {
            Ok(text) => text,
            Err(error) => {
                log::warn!(
                    "[chat-send] parked send {} payload could not be resolved: {error}",
                    delivery_id
                );
                return Err(UserDrain::ZeroWriteRefusal { dispatch_generation });
            }
        };
        let record = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return Ok(());
            };
            if tab.boot_stage != Some(codeterm_types::pty::BootStage::AgentReady)
                || self.pending_delivery_payloads.user_sends.tab(tab_id).revoked.load(Ordering::Acquire) {
                return Ok(());
            }
            take_parked_user_send(&mut tab.pending_delivery, &record_id, send_id.as_deref())
        };
        let Some(record) = record else {
            return Ok(());
        };
        if let Err(message) = super::session_persist::persist_session_after_mutation(
            self,
            super::session_persist::SessionPersistUrgency::Prompt,
        ) {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                if tab.pending_delivery.is_none() {
                    tab.pending_delivery = Some(record);
                }
            }
            log::warn!(
                "[chat-send] parked send {} claim was not persisted: {message}",
                delivery_id
            );
            return Err(UserDrain::ZeroWriteRefusal { dispatch_generation });
        }
        let dispatch_generation = self.pending_delivery_payloads.user_sends.tab(tab_id)
            .dispatch_generation.load(Ordering::Acquire);
        let outcome = if self.pending_delivery_payloads.user_sends.tab(tab_id).revoked.load(Ordering::Acquire) {
            Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "bootstrap owner revoked before handoff".into() })
        } else {
            self.submit_parked_bootstrap(tab_id, &text)
        };
        if let Err(error) = outcome {
            log::warn!(
                "[chat-send] parked send {} for Tab {tab_id} failed: {}",
                delivery_id,
                error.code()
            );
            if !error.bytes_written() {
                let restored = {
                    let mut mux = self.mux.write();
                    mux.get_tab_mut(tab_id).is_some_and(|tab| {
                        return_after_send_failure(
                            &mut tab.pending_delivery,
                            record,
                            error.bytes_written(),
                        )
                        .is_ok()
                    })
                };
                if restored {
                    if let Err(message) = super::session_persist::persist_session_after_mutation(
                        self,
                        super::session_persist::SessionPersistUrgency::Prompt,
                    ) {
                        log::warn!(
                            "[chat-send] parked send {} retry state was not persisted: {message}",
                            delivery_id
                        );
                    }
                } else {
                    log::error!(
                        "[chat-send] parked send {} pre-write failure could not be restored",
                        delivery_id
                    );
                }
                return Err(UserDrain::ZeroWriteRefusal { dispatch_generation });
            }
        }
        let digest_is_live =
            super::pending_delivery_payloads::live_payload_digests(&self.mux.read().snapshot())
                .contains(&record.content_digest);
        if !digest_is_live {
            let mut consumed = Some(record);
            if let Err(error) = self
                .pending_delivery_payloads
                .free_terminal_pair(&mut consumed, "interactive_send_claimed")
            {
                log::warn!(
                    "[chat-send] parked send {} payload cleanup failed: {error}",
                    delivery_id
                );
            }
        }
        Ok(())
    }

    fn submit_parked_bootstrap(&self, tab_id: &str, text: &str) -> Result<(), DeliveryError> {
        #[cfg(test)]
        if let Some(boundary) = tests::bootstrap_boundary(tab_id) { return boundary(text); }
        self.send_chat_text_claimed(tab_id, text, None)
    }

    fn user_readiness_is_current(&self, authority: &UserSendAuthority, readiness: &UserReadiness) -> bool {
        self.user_authority_matches_launch_current(authority)
            && self.user_authority_pty_generation_is_current(authority)
            && self.user_delivery_observation_is_current(readiness.current_observation())
    }

    fn user_send_readiness_is_current(
        &self,
        authority: &UserSendAuthority,
        readiness: Option<&UserReadiness>,
    ) -> bool {
        self.user_authority_is_ready(authority)
            || readiness.is_some_and(|readiness| self.user_readiness_is_current(authority, readiness))
    }

    fn user_authority_matches_launch_current(&self, authority: &UserSendAuthority) -> bool {
        let owner = self.pending_delivery_payloads.user_sends.tab(&authority.tab_id);
        !owner.revoked.load(Ordering::Acquire)
            && !owner.failed.load(Ordering::Acquire)
            && self.user_authority_pty_generation_is_current(authority)
            && self.mux.read().get_tab(&authority.tab_id)
                .is_some_and(|tab| self.user_authority_matches_launch(tab, authority))
    }

    fn submit_owned_user(
        &self,
        record: &UserSendRecord,
        text: &str,
        readiness: Option<&UserReadiness>,
    ) -> Result<(), DeliveryError> {
        let authority = &record.authority;
        let tab_id = authority.tab_id.as_str();
        if !self.user_send_readiness_is_current(authority, readiness) {
            return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner is not ready for this launch".into() });
        }
        if !self.user_authority_is_ready(authority) && !self.rearm_user_input_for_authority(authority) {
            return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner readiness changed before guarded input".into() });
        }
        // Resolved after readiness was observed, so a hook installed before readiness is never missed.
        let permit = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or_else(|| DeliveryError::TabBusyRefused {
                tab: tab_id.into(), detail: "user owner tab closed".into(),
            })?;
            if tab.pty_id.as_deref() != Some(authority.pty_id.as_str())
                || authority.pty_session_generation.is_none_or(|generation| {
                    !self.pty.lock().session_generation_matches(&authority.pty_id, generation)
                })
                || tab.session_binding_nonce.as_deref() != Some(authority.binding_nonce.as_str())
                || tab.agent_type.as_deref() != Some(authority.provider_id.as_str())
                || self.pending_delivery_payloads.user_sends.tab(tab_id).revoked.load(Ordering::Acquire) {
                return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner launch changed".into() });
            }
            self.pty.lock().issue_guarded_input_permit(&authority.pty_id).map_err(|detail| DeliveryError::PtyUnavailable {
                tab: tab_id.into(), detail,
            })?
        };
        let strategy = self.agent_registry.with_provider(&authority.provider_id, |provider| provider.verify_submit_strategy())
            .ok_or_else(|| DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner provider unavailable".into() })?;
        let track_turn = !text.is_empty() && matches!(&strategy, crate::agent::provider::VerifySubmitStrategy::TuiInputDrained { .. });
        if !self.user_send_readiness_is_current(authority, readiness) {
            return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner revoked before submit cycle".into() });
        }
        let claim = self.chat_send_ledger.take_user_submit_claim(&record.send_id)
            .unwrap_or_else(|| super::pty_io::submit_transaction::claim_submit_cycle(&authority.pty_id));
        if !self.user_send_readiness_is_current(authority, readiness) {
            return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner readiness changed before payload write".into() });
        }
        let service = self.clone();
        let guard_authority = authority.clone();
        let pending_record = record.clone();
        let readiness = readiness.cloned().map(std::sync::Arc::new);
        let pre_write_guard: std::sync::Arc<dyn Fn() -> bool + Send + Sync> =
            std::sync::Arc::new(move || {
                let current = service.user_send_readiness_is_current(&guard_authority, readiness.as_deref());
                if !current {
                    service.pending_user_send(&pending_record, "readiness_changed_before_payload_write");
                }
                current
            });
        #[cfg(test)]
        if let Some(boundary) = tests::submit_boundary(tab_id) {
            tests::run_before_submit(tab_id);
            if !pre_write_guard() {
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.into(),
                    detail: "user owner authority changed before payload write".into(),
                });
            }
            let outcome = boundary(record, text);
            if outcome.as_ref().err().is_none_or(DeliveryError::bytes_written) {
                self.mark_user_send_written(record);
            }
            return outcome;
        }
        let write_service = self.clone();
        let write_record = record.clone();
        let payload_queued: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || {
            write_service.mark_user_send_written(&write_record);
        });
        super::pty_io::submit_transaction::submit_text_with_key_claimed_and_follow_up_revalidated(
            self, &authority.pty_id, text, record.typed_follow_up.as_deref(), true, b"\r", strategy,
            super::inject_verify::VerifyOpts { require_prompt_first: false, verify_echo: true, verify_submit: true, ..Default::default() },
            Some(permit), track_turn, claim, Some(pre_write_guard),
            true, Some(payload_queued),
        ).and_then(|result| {
            let elapsed = result.elapsed;
            result.require_submission(track_turn, elapsed, String::new()).map(|_| ())
        })
            .map_err(|error| match error {
                super::inject_verify::InjectionError::AgentTuiPresent { .. } => DeliveryError::TabBusyRefused {
                    tab: tab_id.into(), detail: "provider-classified modal dialog is visible".into(),
                },
                error => DeliveryError::from_injection(tab_id, error),
            })
    }

    fn mark_user_send_written(&self, record: &UserSendRecord) {
        if let Some(status) = self.chat_send_ledger.mark_user_written(&record.send_id) {
            crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
