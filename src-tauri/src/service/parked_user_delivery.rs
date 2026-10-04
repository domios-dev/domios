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
    ZeroWriteRefusal,
}

impl super::ServiceContext {
    pub(crate) fn user_send_authority(&self, tab_id: &str) -> Result<UserSendAuthority, String> {
        let mux = self.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("user send tab is absent")?;
        if !matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty) {
            return Err("user send owner requires PTY backing".into());
        }
        Ok(UserSendAuthority {
            tab_id: tab_id.to_string(),
            pty_id: tab.pty_id.clone().ok_or("user send has no exact PTY authority")?,
            binding_nonce: tab.session_binding_nonce.clone().ok_or("user send has no launch nonce")?,
            provider_id: tab.agent_type.clone().ok_or("user send has no agent identity")?,
            session_id: tab.launch_session_authenticated().then(|| tab.agent_session_id.clone()).flatten(),
            observed_session_id: tab.agent_session_id.clone(),
        })
    }

    pub(crate) fn user_authority_is_ready(&self, authority: &UserSendAuthority) -> bool {
        let owner = self.pending_delivery_payloads.user_sends.tab(&authority.tab_id);
        if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) { return false; }
        let mux = self.mux.read();
        mux.get_tab(&authority.tab_id).is_some_and(|tab| {
            matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty)
                && tab.pty_id.as_deref() == Some(authority.pty_id.as_str())
                && tab.session_binding_nonce.as_deref() == Some(authority.binding_nonce.as_str())
                && tab.agent_type.as_deref() == Some(authority.provider_id.as_str())
                && tab.launch_session_authenticated()
                && authority.session_id.as_deref().is_none_or(|session| tab.agent_session_id.as_deref() == Some(session))
                && matches!(tab.boot_stage, Some(codeterm_types::pty::BootStage::AgentReady | codeterm_types::pty::BootStage::PtyReattached))
        })
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
            Ok::<_, String>(())
        })();
        if let Err(error) = result { log::warn!("[chat-send] tab={tab_id} admission held: {error}"); }
    }

    pub(crate) fn wake_parked_user_send(&self, tab_id: &str) {
        let owner = self.pending_delivery_payloads.user_sends.tab(tab_id);
        owner.wake_generation.fetch_add(1, Ordering::AcqRel);
        if owner.revoked.load(Ordering::Acquire)
            || owner.running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return;
        }
        let service = self.clone();
        let tab_id = tab_id.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            loop {
                let mut generation = owner.wake_generation.load(Ordering::Acquire);
                match service.drain_user_owner(&tab_id) {
                    Ok(UserDrain::ZeroWriteRefusal) => generation = owner.wake_generation.load(Ordering::Acquire),
                    Ok(UserDrain::Idle) => {},
                    Err(error) => log::warn!("[chat-send] tab={tab_id} owner held: {error}"),
                }
                if !owner.revoked.load(Ordering::Acquire)
                    && owner.wake_generation.load(Ordering::Acquire) != generation { continue; }
                owner.running.store(false, Ordering::Release);
                if owner.revoked.load(Ordering::Acquire)
                    || owner.wake_generation.load(Ordering::Acquire) == generation
                    || owner.running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                    break;
                }
            }
        });
    }

    pub(crate) fn deliver_parked_user_send(&self, tab_id: &str) {
        self.wake_parked_user_send(tab_id);
    }

    pub(crate) fn drain_user_owner(&self, tab_id: &str) -> Result<UserDrain, String> {
        if self.pending_delivery_payloads.user_sends.tab(tab_id).revoked.load(Ordering::Acquire) {
            return Ok(UserDrain::Idle);
        }
        if let Err(outcome) = self.deliver_parked_bootstrap_record(tab_id) { return Ok(outcome); }
        #[cfg(test)]
        if let Some(boundary) = tests::submit_boundary(tab_id) {
            return self.drain_user_owner_with(tab_id, |record, text| boundary(record, text));
        }
        self.drain_user_owner_with(tab_id, |record, text| self.submit_owned_user(record, text))
    }

    fn drain_user_owner_with(
        &self,
        tab_id: &str,
        mut submit: impl FnMut(&UserSendRecord, &str) -> Result<(), DeliveryError>,
    ) -> Result<UserDrain, String> {
        let store = &self.pending_delivery_payloads.user_sends;
        for _ in 0..super::pending_delivery_payloads::user_sends::MAX_UNSETTLED * 2 {
            let Some(head) = store.head(tab_id)? else { return Ok(UserDrain::Idle); };
            if !self.user_authority_is_ready(&head.authority) { return Ok(UserDrain::Idle); }
            if head.state == UserSendState::Uncertain {
                self.reconcile_chat_send_evidence(tab_id);
                let current_session = self.mux.read().get_tab(tab_id).and_then(|tab| tab.agent_session_id.clone());
                if current_session != head.handoff_session || !head.cycle_settled
                    || !self.chat_send_ledger.user_confirmed(&head.send_id) || !store.complete(&head)? {
                    return Ok(UserDrain::Idle);
                }
                self.chat_send_ledger.release_user(&head.send_id);
                continue;
            }
            if !store.eligible(&head) { return Ok(UserDrain::Idle); }
            let text = store.prepared(&head)?;
            let evidence = self.chat_send_anchor(tab_id);
            let Some(binding) = evidence.live_binding.as_ref() else { return Ok(UserDrain::Idle); };
            let Some(session) = binding.session_id.as_deref() else { return Ok(UserDrain::Idle); };
            if evidence.anchor.as_ref().is_some_and(|anchor| anchor.provider_id != head.authority.provider_id)
                || binding.binding_nonce.as_deref() != Some(head.authority.binding_nonce.as_str())
                || !self.user_authority_is_ready(&head.authority) { return Ok(UserDrain::Idle); }
            let turn_text = format!("{text}{}", head.typed_follow_up.as_deref().unwrap_or_default());
            if !store.claim(&head, session)? { return Ok(UserDrain::Idle); }
            if !self.chat_send_ledger.begin_user_handoff(&head, &turn_text, evidence) {
                return Err("persisted handoff has no joined ledger admission; held uncertainty".into());
            }
            let claimed = store.head(tab_id)?.ok_or("durable user claim disappeared")?;
            let outcome = if self.user_authority_is_ready(&claimed.authority) {
                submit(&claimed, &text)
            } else {
                Err(DeliveryError::TabBusyRefused { tab: tab_id.to_string(), detail: "user owner authority revoked before handoff".into() })
            };
            let zero_write = outcome.as_ref().err().is_some_and(|error| !error.bytes_written())
                && !self.chat_send_ledger.user_confirmed(&head.send_id);
            store.settle_cycle(&head, zero_write)?;
            if zero_write { self.chat_send_ledger.zero_write_user_handoff(&head.send_id); }
            if let Some(status) = self.chat_send_ledger.finish(&head.send_id, outcome.map_err(|error| error.ledger_message())) {
                crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
            }
            if zero_write { return Ok(UserDrain::ZeroWriteRefusal); }
        }
        Ok(UserDrain::Idle)
    }

    fn deliver_parked_bootstrap_record(&self, tab_id: &str) -> Result<(), UserDrain> {
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
                return Err(UserDrain::ZeroWriteRefusal);
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
            return Err(UserDrain::ZeroWriteRefusal);
        }
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
                return Err(UserDrain::ZeroWriteRefusal);
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

    fn submit_owned_user(&self, record: &UserSendRecord, text: &str) -> Result<(), DeliveryError> {
        let authority = &record.authority;
        let tab_id = authority.tab_id.as_str();
        if !self.user_authority_is_ready(authority) {
            return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner is not ready for this launch".into() });
        }
        let permit = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or_else(|| DeliveryError::TabBusyRefused {
                tab: tab_id.into(), detail: "user owner tab closed".into(),
            })?;
            if tab.pty_id.as_deref() != Some(authority.pty_id.as_str())
                || tab.session_binding_nonce.as_deref() != Some(authority.binding_nonce.as_str())
                || tab.agent_session_id != record.handoff_session
                || tab.agent_type.as_deref() != Some(authority.provider_id.as_str())
                || !tab.launch_session_authenticated()
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
        let session_matches = self.mux.read().get_tab(tab_id)
            .is_some_and(|tab| tab.agent_session_id == record.handoff_session);
        if !self.user_authority_is_ready(authority) || !session_matches {
            return Err(DeliveryError::TabBusyRefused { tab: tab_id.into(), detail: "user owner revoked before submit cycle".into() });
        }
        let claim = super::pty_io::submit_transaction::claim_submit_cycle(&authority.pty_id);
        super::pty_io::submit_transaction::submit_text_with_key_claimed_and_follow_up(
            self, &authority.pty_id, text, record.typed_follow_up.as_deref(), true, b"\r", strategy,
            super::inject_verify::VerifyOpts { require_prompt_first: false, verify_echo: true, verify_submit: true, ..Default::default() },
            Some(permit), track_turn, claim,
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
}

#[cfg(test)]
pub(crate) mod tests;
